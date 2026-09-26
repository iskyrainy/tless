use std::{
    collections::HashMap,
    fmt::Display,
    sync::{Arc, LazyLock},
};

use anyhow::{Context, Error, Result, bail};
use futures::{StreamExt, stream};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::{
    fs::{self, File},
    io::AsyncWriteExt,
    sync::RwLock,
};
use tracing::{info, warn};

use crate::{
    error,
    server::{SITE, get_source_path},
    util::get_cpu,
};

static HTTP_CLIENT: LazyLock<Client> = LazyLock::new(Client::new);

struct ProviderInfo {
    api_key: String,
    model: String,
}

trait Provider: Send + Sync {
    fn base_url(&self) -> &str;
    fn api_key(&self) -> &str;
    fn model(&self) -> &str;
}

macro_rules! make_provider {
    ($provider: ident, $url: literal) => {
        impl Provider for $provider {
            fn base_url(&self) -> &str {
                $url
            }

            fn api_key(&self) -> &str {
                &self.0.api_key
            }

            fn model(&self) -> &str {
                &self.0.model
            }
        }
    };
}

const SYSTEM_PROMPT: &str = r#"
You are a professional technical translator for a static site blog. Your task is to translate the provided Markdown content into the target language specified by the user.

## Core rules
1. Preserve the Markdown structure exactly. Keep all headings, lists, links, images, code blocks, inline code, tables, and blockquotes intact. Do not add, remove, or reorder any structural element.
2. Preserve the front matter exactly as it is, except for the fields explicitly listed as translatable below. Do not translate or modify field names.
3. Translatable front matter fields: title, description, summary, excerpt. All other fields (slug, date, tags, categories, draft, weight, aliases, and any custom fields) must remain byte-for-byte identical.
4. Do not translate content inside code blocks or inline code. Code, file paths, command names, API names, and identifiers must remain unchanged.
5. Follow the provided glossary strictly. If a term appears in the glossary, always use the specified translation. If no glossary entry exists, prefer the most widely accepted technical translation in the target language.
6. Match the tone and register of the source. Content should read naturally in the target language, not as a literal word-for-word conversion.
7. Output only the translated content. Do not add explanations, prefaces, apologies, or notes. Do not wrap the output in a code fence.
8. If a sentence is ambiguous or a term has no established translation, translate it in the most natural way and keep it consistent throughout the document.
9. There has two user messages, one is the origin Markdown content and another one is the specified target language. Translate only the content of Markdown, and output only the translated Markdown. Do not include the other part of input in your output.
"#;

#[async_trait::async_trait]
trait TranslationBackend: Send + Sync {
    async fn translate(&self, origin_text: &str, target_lang: &str) -> Result<String>;
}

macro_rules! make_translate {
    ($provider: ident) => {
        #[async_trait::async_trait]
        impl TranslationBackend for $provider {
            async fn translate(&self, origin_text: &str, target_lang: &str) -> Result<String> {
                let client = HTTP_CLIENT.clone();
                let payload = serde_json::json!({
                    "model": self.model(),
                    "messages": [
                        {
                            "role": "system",
                            "content": [{"type": "text", "text": SYSTEM_PROMPT}]
                        },
                        {
                            "role": "user",
                            "content": [{
                                "type": "text",
                                "text": origin_text
                            }]
                        },
                        {
                            "role": "user",
                            "content": [{
                                "type": "text",
                                "text": target_lang
                            }]
                        },
                    ],
                    "stream": false,
                    "thinking": {
                        "type": "disabled"
                    },
                });

                match client
                    .post(self.base_url())
                    .header("Content-Type", "application/json")
                    .header("Accept", "application/json")
                    .header("Authorization", format!("Bearer {}", self.api_key()))
                    .json(&payload)
                    .send()
                    .await
                {
                    Ok(resp) => {
                        if !resp.status().is_success() {
                            let status_code = resp.status();
                            let body = resp.text().await.unwrap_or_default();
                            bail!("Failed to request llm api {status_code}: {body}");
                        }
                        match resp.text().await {
                            Ok(data) => match serde_json::from_str::<Value>(&data) {
                                Ok(data) => Ok(data["choices"][0]["message"]["content"]
                                    .as_str()
                                    .unwrap_or("")
                                    .to_string()),
                                Err(e) => bail!("Failed to deserialize resp text: {e}"),
                            },
                            Err(e) => bail!("Failed to fetch resp text: {e}"),
                        }
                    }
                    Err(e) => bail!("Failed to request llm api: {e}"),
                }
            }
        }

    };
}

enum Providers {
    Deepseek,
    Kimi,
    Glm,
}

impl Providers {
    fn create(provider: &str) -> Result<Self> {
        let s = match provider {
            "deepseek" => Self::Deepseek,
            "kimi" => Self::Kimi,
            "glm" => Self::Glm,
            _ => bail!("Get not support provider, expected <-- deepseek | kimi | glm -->"),
        };
        Ok(s)
    }

    fn into_backend(self, api_key: &str, model: &str) -> Box<dyn TranslationBackend> {
        let info = ProviderInfo {
            api_key: api_key.to_owned(),
            model: model.to_owned(),
        };
        match self {
            Providers::Deepseek => Box::new(DeepseekProvider(info)),
            Providers::Kimi => Box::new(KimiProvider(info)),
            Providers::Glm => Box::new(GlmProvider(info)),
        }
    }
}

struct DeepseekProvider(ProviderInfo);
struct KimiProvider(ProviderInfo);
struct GlmProvider(ProviderInfo);

make_provider!(
    DeepseekProvider,
    "https://api.deepseek.com/chat/completions"
);
make_provider!(KimiProvider, "https://api.moonshot.cn/v1/chat/completions");
make_provider!(
    GlmProvider,
    "https://open.bigmodel.cn/api/paas/v4/chat/completions"
);

make_translate!(DeepseekProvider);
make_translate!(KimiProvider);
make_translate!(GlmProvider);

pub async fn translate() -> Result<()> {
    let site = SITE.load();
    let p =
        Providers::create(&site.i18n.provider)?.into_backend(&site.i18n.api_key, &site.i18n.model);

    let mut tls = vec![];
    for tl in site.get_i18n_tl() {
        if let Some(tl) = Language::from_code(tl) {
            tls.push(tl);
        } else {
            warn!("Not a standard target language: {tl}");
        }
    }
    translate_tls(&*p, tls).await?;
    dump_hash().await?;
    Ok(())
}

async fn translate_tls(
    provider: &dyn TranslationBackend,
    target_langs: Vec<Language>,
) -> Result<()> {
    let provider = Arc::new(provider);
    let site = SITE.load();
    let target_langs = Arc::new(target_langs);

    stream::iter(&site.post)
        .map(|d| {
            let p = provider.clone();
            let tls = target_langs.clone();
            async move {
                let Some(name) = d.path.file_name().and_then(|s| s.to_str()) else {
                    return Ok(());
                };

                let md_str = fs::read_to_string(&d.path).await.context(format!(
                    "Failed to read origin markdown: {}",
                    d.path.display()
                ))?;
                let new = compute_md5(&md_str);
                let name_key = name.to_string();
                if let Some(old) = POST_HASH.read().await.get(&name_key)
                    && old.eq(&new)
                {
                    return Ok(());
                }
                POST_HASH.write().await.insert(name_key, new);

                for target_lang in tls.iter() {
                    let dst_dir = get_source_path("i18n").join(target_lang.get_str_value());
                    if !dst_dir.exists() {
                        fs::create_dir_all(&dst_dir)
                            .await
                            .context(format!("Failed to create i18n dir: {}", dst_dir.display()))?;
                    }
                    let dst_file = dst_dir.join(name);
                    let mut file = File::create(&dst_file).await.context(format!(
                        "Failed to create translated markdown: {}",
                        dst_file.display()
                    ))?;

                    let target_str = p.translate(&md_str, &target_lang.get_str()).await?;
                    file.write_all_buf(&mut target_str.as_bytes())
                        .await
                        .context(format!(
                            "Failed to write translated markdown: {}",
                            dst_file.display()
                        ))?;
                    file.flush().await.context(format!(
                        "Failed to flush translated markdown: {}",
                        dst_file.display()
                    ))?;
                }
                info!("Translate {name} finished");

                Ok(())
            }
        })
        .buffer_unordered(get_cpu())
        .collect::<Vec<Result<(), Error>>>()
        .await
        .into_iter()
        .collect::<_>()
}

async fn dump_hash() -> Result<()> {
    let path = get_source_path("post").join(".post_hash.json");
    let map = POST_HASH.read().await;
    fs::write(
        &path,
        serde_json::to_string(&*map).context("Failed to serialize map into string")?,
    )
    .await
    .context("Failed to dump posts md5 value into source/post/.post_hash.json")?;
    Ok(())
}

static POST_HASH: LazyLock<RwLock<HashMap<String, String>>> = LazyLock::new(|| {
    let path = get_source_path("post").join(".post_hash.json");
    if path.exists() {
        let hash_str = std::fs::read_to_string(&path)
            .unwrap_or_else(|_| error::fatal("Failed to read source/post/.post_hash.json"));
        RwLock::new(
            serde_json::from_str::<HashMap<String, String>>(&hash_str).unwrap_or_else(|_| {
                error::fatal("Failed to deserialize source/post/.post_hash.json")
            }),
        )
    } else {
        RwLock::new(HashMap::new())
    }
});

#[inline]
pub fn compute_md5(text: &String) -> String {
    let digest = md5::compute(text);
    format!("{:x}", digest)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Language {
    #[serde(rename = "af")]
    Afrikaans,
    #[serde(rename = "sq")]
    Albanian,
    #[serde(rename = "am")]
    Amharic,
    #[serde(rename = "ar")]
    Arabic,
    #[serde(rename = "hy")]
    Armenian,
    #[serde(rename = "as")]
    Assamese,
    #[serde(rename = "az")]
    Azerbaijani,
    #[serde(rename = "eu")]
    Basque,
    #[serde(rename = "bn")]
    Bengali,
    #[serde(rename = "bg")]
    Bulgarian,
    #[serde(rename = "my")]
    Burmese,
    #[serde(rename = "ca")]
    Catalan,
    #[serde(rename = "chr")]
    Cherokee,
    #[serde(rename = "zh-HK")]
    ChineseHongKong,
    #[serde(rename = "zh-CN")]
    ChineseSimplified,
    #[serde(rename = "zh-TW")]
    ChineseTraditional,
    #[serde(rename = "hr")]
    Croatian,
    #[serde(rename = "cs")]
    Czech,
    #[serde(rename = "da")]
    Danish,
    #[serde(rename = "nl")]
    Dutch,
    #[serde(rename = "en-GB")]
    EnglishUk,
    #[serde(rename = "en")]
    EnglishUs,
    #[serde(rename = "et")]
    Estonian,
    #[serde(rename = "fil")]
    Filipino,
    #[serde(rename = "fi")]
    Finnish,
    #[serde(rename = "fr")]
    French,
    #[serde(rename = "fr-CA")]
    FrenchCanada,
    #[serde(rename = "gl")]
    Galician,
    #[serde(rename = "ka")]
    Georgian,
    #[serde(rename = "de")]
    German,
    #[serde(rename = "el")]
    Greek,
    #[serde(rename = "gu")]
    Gujarati,
    #[serde(rename = "iw")]
    Hebrew,
    #[serde(rename = "hi")]
    Hindi,
    #[serde(rename = "hu")]
    Hungarian,
    #[serde(rename = "is")]
    Icelandic,
    #[serde(rename = "id")]
    Indonesian,
    #[serde(rename = "ga")]
    Irish,
    #[serde(rename = "it")]
    Italian,
    #[serde(rename = "ja")]
    Japanese,
    #[serde(rename = "kn")]
    Kannada,
    #[serde(rename = "kk")]
    Kazakh,
    #[serde(rename = "km")]
    Khmer,
    #[serde(rename = "ko")]
    Korean,
    #[serde(rename = "lo")]
    Lao,
    #[serde(rename = "lv")]
    Latvian,
    #[serde(rename = "lt")]
    Lithuanian,
    #[serde(rename = "mk")]
    Macedonian,
    #[serde(rename = "ms")]
    Malay,
    #[serde(rename = "ml")]
    Malayalam,
    #[serde(rename = "mr")]
    Marathi,
    #[serde(rename = "mn")]
    Mongolian,
    #[serde(rename = "ne")]
    Nepali,
    #[serde(rename = "no")]
    Norwegian,
    #[serde(rename = "or")]
    Oriya,
    #[serde(rename = "fa")]
    Persian,
    #[serde(rename = "pl")]
    Polish,
    #[serde(rename = "pt-BR")]
    PortugueseBrazil,
    #[serde(rename = "pt-PT")]
    PortuguesePortugal,
    #[serde(rename = "pa")]
    Punjabi,
    #[serde(rename = "ro")]
    Romanian,
    #[serde(rename = "ru")]
    Russian,
    #[serde(rename = "sr")]
    Serbian,
    #[serde(rename = "si")]
    Sinhala,
    #[serde(rename = "sk")]
    Slovak,
    #[serde(rename = "sl")]
    Slovenian,
    #[serde(rename = "es")]
    Spanish,
    #[serde(rename = "es-419")]
    SpanishLatinAmerica,
    #[serde(rename = "sw")]
    Swahili,
    #[serde(rename = "sv")]
    Swedish,
    #[serde(rename = "ta")]
    Tamil,
    #[serde(rename = "te")]
    Telugu,
    #[serde(rename = "th")]
    Thai,
    #[serde(rename = "tr")]
    Turkish,
    #[serde(rename = "uk")]
    Ukrainian,
    #[serde(rename = "ur")]
    Urdu,
    #[serde(rename = "uz")]
    Uzbek,
    #[serde(rename = "vi")]
    Vietnamese,
    #[serde(rename = "cy")]
    Welsh,
    #[serde(rename = "zu")]
    Zulu,
}

impl Language {
    pub fn from_code(code: &str) -> Option<Self> {
        serde_json::from_value(Value::String(code.to_string())).ok()
    }

    #[inline]
    pub fn get_str_value(&self) -> String {
        serde_json::to_value(self)
            .ok()
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_default()
    }

    #[inline]
    pub fn get_str(&self) -> String {
        format!("{:?}", self)
    }
}

impl TryFrom<&str> for Language {
    type Error = anyhow::Error;
    fn try_from(value: &str) -> std::result::Result<Self, Self::Error> {
        Ok(serde_json::from_value::<Language>(Value::String(
            value.to_string(),
        ))?)
    }
}

impl From<Language> for String {
    fn from(value: Language) -> Self {
        serde_json::to_value(value)
            .ok()
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_default()
    }
}

impl Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.get_str())
    }
}

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
# Role
You are a professional technical translator specializing in software engineering, programming, and technical blogs. Your task is to translate the provided Markdown document into the user-specified target language with complete accuracy, consistency, and fluency.

There has two user messages, one is the origin Markdown content and another one is the specified target language.

# Translation Rules
1. **Complete Translation (Highest Priority)**
   * Translate the entire source document without omitting, skipping, summarizing, or shortening any translatable content.
   * Translate every paragraph, sentence, heading, list item, table cell, blockquote, caption, footnote, and other user-visible text.
   * Process the document sequentially from beginning to end, ensuring that no content is overlooked, including the final paragraphs and trailing sections.
   * Preserve the original meaning, technical details, examples, explanations, and level of detail. Never replace the original content with a summary or paraphrase that loses information.
   * Before outputting, verify that every translatable segment in the source has a corresponding translation.
2. **Markdown Structure Preservation**
   * Preserve the original Markdown structure, element order, heading levels, list nesting, table dimensions, and paragraph boundaries.
   * Do not add, remove, merge, split, or reorder structural elements.
   * Preserve Markdown syntax, formatting, blank lines, and document organization.
   * Translate human-readable text, including link labels and image alt text, while keeping Markdown syntax and resource references intact.
3. **Front Matter Preservation**
   * Preserve the original Front Matter format, field order, field names, delimiters, and formatting.
   * Translate only the values of `title`, `description`, `summary`, and `excerpt`.
   * Keep all other fields, including `slug`, `date`, `tags`, `categories`, `layout`, `weight`, `aliases`, and custom fields, exactly unchanged.
   * Never modify machine-readable values, identifiers, URLs, or template expressions.
4. **Code and Technical Content Protection**
   * Never translate or modify fenced code blocks, inline code, commands, programming syntax, identifiers, file paths, API names, URLs, or other machine-readable content.
   * Preserve code indentation, language identifiers, mathematical expressions, and technical notation exactly.
   * Translate only natural-language explanations surrounding protected content.
   * Preserve HTML tags, attributes, embedded scripts, styles, and template syntax, translating only clearly identifiable user-visible text.
5. **Terminology and Consistency**
   * Strictly follow the provided glossary. Glossary translations take precedence over general terminology preferences.
   * Use widely accepted technical terminology when no glossary entry exists.
   * Translate recurring terms consistently throughout the entire document.
   * Use the surrounding context to resolve ambiguity and preserve the original technical meaning.
6. **Writing Quality**
   * Produce natural, fluent, grammatically correct, and professional technical writing in the target language.
   * Preserve the source's tone, style, intent, technical precision, and level of detail.
   * Avoid literal, awkward, or unnecessarily verbose translations.
   * Do not introduce new information, remove details, correct technical content, or alter the author's intent.

# Completeness Validation
Before returning the translation, silently perform a complete source-to-output verification:
* Ensure every translatable section, paragraph, sentence, list item, table cell, and other textual element has been translated.
* Ensure no content has been skipped, truncated, duplicated, summarized, or unintentionally altered.
* Ensure the beginning, middle, and end of the document are all fully translated.
* Ensure all protected code, metadata, links, and structural elements remain intact.
* Ensure terminology and translations are consistent throughout the document.

**Completeness is mandatory. Translation fluency must never come at the expense of content coverage or technical accuracy. If the document is too long to translate reliably in one response, do not silently omit or truncate content.**

# Input Convention
The user provides the source Markdown and the target language in separate messages. Additional messages may contain a glossary or translation requirements.

Identify each input by its purpose. Translate only the source Markdown into the specified target language. Never include instructions, glossary content, or unrelated messages in the translation.

# Output Requirements
* Output only the complete translated Markdown document.
* Do not include explanations, introductions, summaries, translator notes, or validation reports.
* Do not wrap the output in code fences.
* Preserve the original document's structure and content coverage.
* Ensure the output is ready to be saved and rendered as a Markdown file.

Return only the translated Markdown content.
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

use std::sync::{Arc, LazyLock};

use anyhow::{Context, Error, Result, bail};
use futures::{StreamExt, stream};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::{
    fs::{self, File},
    io::AsyncWriteExt,
};

use crate::server::{SITE, get_source_path};

static HTTP_CLIENT: LazyLock<Client> = LazyLock::new(Client::new);

struct ProviderInfo {
    api_key: String,
    model: String,
}

trait Provider: Send + Sync {
    fn name(&self) -> &str;
    fn base_url(&self) -> &str;
    fn api_key(&self) -> &str;
    fn model(&self) -> &str;
}

macro_rules! make_provider {
    ($provider: ident, $name: literal, $url: literal) => {
        impl Provider for $provider {
            fn name(&self) -> &str {
                $name
            }

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
    OpenAI,
    Deepseek,
    Qwen,
    Kimi,
    Glm,
}

impl Providers {
    fn create(provider: &str) -> Result<Self> {
        let s = match provider {
            "openai" => Self::OpenAI,
            "deepseek" => Self::Deepseek,
            "qwen" => Self::Qwen,
            "kimi" => Self::Kimi,
            "glm" => Self::Glm,
            _ => bail!(
                "Get not support provider, expected <-- openai | deepseek | qwen | kimi |glm -->"
            ),
        };
        Ok(s)
    }

    fn into_backend(self, api_key: &str, model: &str) -> Box<dyn TranslationBackend> {
        let info = ProviderInfo {
            api_key: api_key.to_owned(),
            model: model.to_owned(),
        };
        match self {
            Providers::OpenAI => Box::new(OpenaiProvider(info)),
            Providers::Deepseek => Box::new(DeepseekProvider(info)),
            Providers::Qwen => Box::new(QwenProvider(info)),
            Providers::Kimi => Box::new(KimiProvider(info)),
            Providers::Glm => Box::new(GlmProvider(info)),
        }
    }
}

struct OpenaiProvider(ProviderInfo);
struct DeepseekProvider(ProviderInfo);
struct QwenProvider(ProviderInfo);
struct KimiProvider(ProviderInfo);
struct GlmProvider(ProviderInfo);

make_provider!(
    OpenaiProvider,
    "openai",
    "https://api.openai.com/v1/responses"
);
make_provider!(
    DeepseekProvider,
    "deepseek",
    "https://api.deepseek.com/chat/completions"
);
make_provider!(
    QwenProvider,
    "qwen",
    "https://dashscope-intl.aliyuncs.com/compatible-mode/v1"
);
make_provider!(
    KimiProvider,
    "kimi",
    "https://api.moonshot.cn/v1/chat/completions"
);
make_provider!(
    GlmProvider,
    "glm",
    "https://open.bigmodel.cn/api/paas/v4/chat/completions"
);

make_translate!(OpenaiProvider);
make_translate!(DeepseekProvider);
make_translate!(QwenProvider);
make_translate!(KimiProvider);
make_translate!(GlmProvider);

pub(crate) async fn translate(
    provider: &str,
    api_key: &str,
    model: &str,
    target_lang: Vec<&str>,
) -> Result<()> {
    let p = Providers::create(provider)?.into_backend(api_key, model);
    for tl in target_lang {
        translate_tl(&*p, tl).await?;
    }
    Ok(())
}

async fn translate_tl(provider: &dyn TranslationBackend, target_lang: &str) -> Result<()> {
    let provider = Arc::new(provider);
    let site = SITE.load();

    // FIXME: this will translate all post, but we only need translate those which are updated. For
    // less llm api cost.
    stream::iter(&site.post)
        .map(|d| {
            let p = provider.clone();
            async move {
                let md_str = fs::read_to_string(&d.path).await.context(format!(
                    "Failed to read origin markdown: {}",
                    &d.path.display()
                ))?;
                let Some(name) = d.path.file_name().and_then(|s| s.to_str()) else {
                    return Ok(());
                };
                // FIXME: target_lang as path is not reasonable. But the problem is how to design
                // target_lang and transfer it from config to llm request and render path.
                let dst = get_source_path("post").join(target_lang).join(name);
                let mut file = File::create(&dst).await.context(format!(
                    "Failed to create translated markdown: {}",
                    &dst.display()
                ))?;
                let target_str = p.translate(&md_str, target_lang).await?;
                file.write_all_buf(&mut target_str.as_bytes())
                    .await
                    .context(format!(
                        "Failed to write translated markdown: {}",
                        &dst.display()
                    ))?;
                file.flush().await.context(format!(
                    "Failed to flush translated markdown: {}",
                    &dst.display()
                ))?;
                Ok(())
            }
        })
        .buffer_unordered(4)
        .collect::<Vec<Result<(), Error>>>()
        .await
        .into_iter()
        .collect::<_>()
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct I18nConfig {
    pub provider: String,
    pub api_key: String,
    pub model: String,
    pub target_lang: Vec<String>,
}

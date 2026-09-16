use std::sync::LazyLock;

use anyhow::{Result, bail};
use reqwest::Client;
use serde_json::Value;

static HTTP_CLIENT: LazyLock<Client> = LazyLock::new(Client::new);

struct ProviderInfo {
    api_key: String,
    model: String,
}

pub(crate) trait Provider {
    const NAME: &'static str;
    const BASE_URL: &'static str;

    fn name(&self) -> &str {
        Self::NAME
    }
    fn base_url(&self) -> &str {
        Self::BASE_URL
    }
    fn api_key(&self) -> &str;
    fn model(&self) -> &str;
    fn client(&self) -> &Client {
        &HTTP_CLIENT
    }
}

macro_rules! make_provider {
    ($provider: ident, $name: literal, $url: literal) => {
        impl $provider {
            fn new(api_key: String, model: String) -> Self {
                Self(ProviderInfo { api_key, model })
            }
        }

        impl Provider for $provider {
            const NAME: &'static str = $name;
            const BASE_URL: &'static str = $url;

            fn name(&self) -> &str {
                Self::NAME
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

pub(crate) trait TranslationBackend {
    const SYSTEM_PROMPT: &'static str = r#"
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
        9. The source content is wrapped in <source>...</source>. The target language is specified in <target_language>...</target_language>. Translate only the content inside <source>, and output only the translated Markdown. Do not include the <source> or <target_language> tags in your output.
    "#;
    async fn translate(&self, origin_text: &str, target_lang: &str) -> Result<String>;
}

macro_rules! make_translate {
    ($provider: ident) => {
        impl TranslationBackend for $provider {
            async fn translate(&self, origin_text: &str, target_lang: &str) -> Result<String> {
                let client = self.client();
                let payload = format!(
                    r#"{{
                        "model": "{}",
                        "messages": [
                            {{
                                "role": "system",
                                "content": [
                                    {{
                                        "type": "text",
                                        "text": "{}"
                                    }}
                                ]
                            }},
                            {{
                                "role": "user",
                                "content": [
                                    {{
                                        "type": "text",
                                        "text": "<source>{}</source><target_language>{}</target_language>"
                                    }}
                                ]
                            }}
                        ],
                        "thinking": {{
                            "type": "disabled"
                        }},
                        "max_tokens": 102400,
                        "response_format": {{
                            "type": "text"
                        }},
                        "stream": false,
                    }}"#,
                    self.model(),
                    Self::SYSTEM_PROMPT,
                    origin_text,
                    target_lang,
                );

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
                            bail!("Failed to request llm api: {}", resp.status());
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

pub(crate) struct OpenaiProvider(ProviderInfo);
pub(crate) struct DeepseekProvider(ProviderInfo);
pub(crate) struct QwenProvider(ProviderInfo);
pub(crate) struct KimiProvider(ProviderInfo);
pub(crate) struct GlmProvider(ProviderInfo);

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

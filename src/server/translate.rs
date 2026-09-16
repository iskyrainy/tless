use std::sync::LazyLock;

use anyhow::{Result, bail};
use reqwest::Client;

trait TranslationBackend {
    const SYSTEM_PROMPT: &'static str = "";
    async fn translate(&self, req: TranslationRequest) -> Result<TranslationResponse>;
}

struct TranslationRequest {
    stable_prefix: String,
    target_lang: String,
}

struct TranslationResponse {
    text: String,
    usage: Option<Usage>,
}

#[derive(Debug)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    pub cached_tokens: u32,
    pub cache_miss_tokens: Option<u32>,
}

static CLIENT: LazyLock<Client> = LazyLock::new(Client::new);

struct ProviderInfo {
    api_key: String,
    model: String,
}

trait Provider {
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
        &CLIENT
    }
}

macro_rules! make_provider {
    ($provider: ident, $name: literal, $url: literal) => {
        impl Provider for $provider {
            const NAME: &'static str = $name;
            const BASE_URL: &'static str = $url;

            fn api_key(&self) -> &str {
                &self.0.api_key
            }
            fn model(&self) -> &str {
                &self.0.model
            }
        }
    };
}

macro_rules! make_translate {
    ($provider: ident) => {
        impl TranslationBackend for $provider {
            async fn translate(&self, req: TranslationRequest) -> Result<TranslationResponse> {
                let client = self.client();
                let payload = format!("");
                if let Ok(resp) = client
                    .post(self.base_url())
                    .header("Content-Type", "application/json")
                    .header("Accept", "application/json")
                    .header("Authorization", format!("Bearer {}", self.api_key()))
                    .json(&payload)
                    .send()
                    .await
                    && resp.status().is_success()
                {}
                todo!()
            }
        }
    };
}

struct OpenaiProvider(ProviderInfo);
struct DeepseekProvider(ProviderInfo);
struct QwenProvider(ProviderInfo);
struct KimiProvider(ProviderInfo);
struct GlmProvider(ProviderInfo);

make_provider!(OpenaiProvider, "openai", "");
make_provider!(
    DeepseekProvider,
    "deepseek",
    "https://api.deepseek.com/chat/completions"
);
make_provider!(QwenProvider, "qwen", "");
make_provider!(KimiProvider, "kimi", "");
make_provider!(GlmProvider, "glm", "");

make_translate!(OpenaiProvider);

make_translate!(QwenProvider);
make_translate!(KimiProvider);
make_translate!(GlmProvider);

impl TranslationBackend for DeepseekProvider {
    async fn translate(&self, req: TranslationRequest) -> Result<TranslationResponse> {
        let client = self.client();
        let payload = format!("");
        if let Ok(resp) = client
            .post(self.base_url())
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .header("Authorization", format!("Bearer {}", self.api_key()))
            .json(&payload)
            .send()
            .await
        {
            if !resp.status().is_success() {
                bail!("Failed to translate markdown");
            }
            if let Ok(data) = resp.text().await {
                
            }
            return Ok(TranslationResponse {
                text: "".to_string(),
                usage: None,
            });
        }
        todo!()
    }
}

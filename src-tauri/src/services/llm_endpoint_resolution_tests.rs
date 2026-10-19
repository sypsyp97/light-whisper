use super::llm_provider::{
    assistant_endpoint_for_config, endpoint_for_config, screen_vision_endpoint_for_config,
    selection_endpoint_for_config, validation_endpoint_for_config, LlmEndpoint,
};
use crate::state::user_profile::{ApiFormat, CustomProvider, LlmProviderConfig};

type ExpectedEndpoint<'a> = (&'a str, &'a str, &'a str, u64, ApiFormat);

const PRESETS: &[(&str, &str, &str, u64)] = &[
    (
        "cerebras",
        "https://api.cerebras.ai/v1/chat/completions",
        "qwen-3.8-27b",
        5,
    ),
    (
        "openai",
        "https://api.openai.com/v1/responses",
        "gpt-6-luna",
        10,
    ),
    ("xai", "https://api.x.ai/v1/responses", "grok-4.7", 10),
    (
        "deepseek",
        "https://api.deepseek.com/v1/chat/completions",
        "deepseek-flash",
        10,
    ),
    (
        "siliconflow",
        "https://api.siliconflow.cn/v1/chat/completions",
        "Qwen/Qwen3.8-27B",
        10,
    ),
    (
        "openrouter",
        "https://openrouter.ai/api/v1/chat/completions",
        "~deepseek/deepseek-flash-latest",
        10,
    ),
    (
        "custom",
        "http://127.0.0.1:8000/v1/chat/completions",
        "gpt-6-luna",
        10,
    ),
];

fn assert_endpoint(actual: LlmEndpoint, expected: ExpectedEndpoint<'_>) {
    assert_eq!(
        (
            actual.provider.as_str(),
            actual.api_url.as_str(),
            actual.model.as_str(),
            actual.timeout_secs,
            actual.api_format,
        ),
        expected,
    );
}

fn custom_provider(id: &str, model: &str, api_format: ApiFormat) -> CustomProvider {
    CustomProvider {
        id: id.to_string(),
        name: "自定义供应商 · β".to_string(),
        base_url: " https://gateway.example/v1/ ".to_string(),
        model: model.to_string(),
        api_format,
    }
}

#[derive(Clone, Copy, Debug)]
enum Role {
    Assistant,
    Selection,
    Validation,
    ScreenVision,
}

const ROLES: [Role; 4] = [
    Role::Assistant,
    Role::Selection,
    Role::Validation,
    Role::ScreenVision,
];

impl Role {
    fn configure(
        self,
        config: &mut LlmProviderConfig,
        enabled: bool,
        provider: Option<&str>,
        model: Option<&str>,
    ) {
        let provider = provider.map(str::to_string);
        let model = model.map(str::to_string);
        match self {
            Self::Assistant => {
                config.assistant_use_separate_model = enabled;
                config.assistant_provider = provider;
                config.assistant_model = model;
            }
            Self::Selection => {
                config.selection_use_separate_model = enabled;
                config.selection_provider = provider;
                config.selection_model = model;
            }
            Self::Validation => {
                config.validation_use_separate_model = enabled;
                config.validation_provider = provider;
                config.validation_model = model;
            }
            Self::ScreenVision => {
                config.screen_vision_enabled = enabled;
                config.screen_vision_provider = provider;
                config.screen_vision_model = model;
            }
        }
    }

    fn resolve(self, config: &LlmProviderConfig) -> Option<LlmEndpoint> {
        match self {
            Self::Assistant => Some(assistant_endpoint_for_config(config)),
            Self::Selection => Some(selection_endpoint_for_config(config)),
            Self::Validation => Some(validation_endpoint_for_config(config)),
            Self::ScreenVision => screen_vision_endpoint_for_config(config),
        }
    }
}

#[test]
fn preset_defaults_and_blank_models_keep_exact_endpoint_fields() {
    for &(provider, url, model, timeout) in PRESETS {
        for custom_model in [None, Some(""), Some(" \t\n")] {
            let config = LlmProviderConfig {
                active: provider.to_string(),
                custom_model: custom_model.map(str::to_string),
                custom_base_url: Some(" \t".to_string()),
                ..Default::default()
            };
            assert_endpoint(
                endpoint_for_config(&config),
                (provider, url, model, timeout, ApiFormat::OpenaiCompat),
            );
        }
    }
}

#[test]
fn preset_models_trim_unicode_overrides_and_only_custom_uses_the_legacy_url() {
    for &(provider, default_url, _, timeout) in PRESETS {
        let config = LlmProviderConfig {
            active: provider.to_string(),
            custom_model: Some(" \t模型/β-2\n".to_string()),
            custom_base_url: Some(" https://legacy.example/api/v3/ ".to_string()),
            ..Default::default()
        };
        let url = if provider == "custom" {
            "https://legacy.example/api/v3/chat/completions"
        } else {
            default_url
        };
        assert_endpoint(
            endpoint_for_config(&config),
            (provider, url, "模型/β-2", timeout, ApiFormat::OpenaiCompat),
        );
    }
}

#[test]
fn custom_entries_ignore_stale_legacy_fields_and_preserve_nonblank_model_whitespace() {
    for (format, url, timeout) in [
        (
            ApiFormat::OpenaiCompat,
            "https://gateway.example/v1/chat/completions",
            10,
        ),
        (
            ApiFormat::Anthropic,
            "https://gateway.example/v1/messages",
            30,
        ),
    ] {
        let config = LlmProviderConfig {
            active: "供应商_β".to_string(),
            custom_model: Some("stale-preset-model".to_string()),
            custom_base_url: Some("https://stale.example".to_string()),
            custom_providers: vec![custom_provider("供应商_β", "  模型/β-2 \t", format.clone())],
            ..Default::default()
        };
        assert_endpoint(
            endpoint_for_config(&config),
            ("供应商_β", url, "  模型/β-2 \t", timeout, format.clone()),
        );
        for role in [Role::Assistant, Role::Selection, Role::Validation] {
            assert_endpoint(
                role.resolve(&config).expect("fallback endpoint"),
                ("供应商_β", url, "  模型/β-2 \t", timeout, format.clone()),
            );
        }
    }
}

#[test]
fn blank_custom_entry_models_use_the_default_instead_of_stale_legacy_models() {
    for (format, url, timeout) in [
        (
            ApiFormat::OpenaiCompat,
            "http://127.0.0.1:8000/v1/chat/completions",
            10,
        ),
        (
            ApiFormat::Anthropic,
            "https://api.anthropic.com/v1/messages",
            30,
        ),
    ] {
        for blank in ["", " \t\n"] {
            let mut custom = custom_provider("custom_a", blank, format.clone());
            custom.base_url = blank.to_string();
            let config = LlmProviderConfig {
                active: "custom_a".to_string(),
                custom_model: Some("stale-preset-model".to_string()),
                custom_providers: vec![custom],
                ..Default::default()
            };
            assert_endpoint(
                endpoint_for_config(&config),
                ("custom_a", url, "gpt-6-luna", timeout, format.clone()),
            );
        }
    }
}

#[test]
fn invalid_active_provider_uses_the_last_custom_entry_or_cerebras() {
    for invalid in ["removed-provider", "", " openai "] {
        let mut config = LlmProviderConfig {
            active: invalid.to_string(),
            custom_model: Some(" legacy-model ".to_string()),
            custom_base_url: Some("https://stale.example".to_string()),
            ..Default::default()
        };
        assert_endpoint(
            endpoint_for_config(&config),
            (
                "cerebras",
                "https://api.cerebras.ai/v1/chat/completions",
                "legacy-model",
                5,
                ApiFormat::OpenaiCompat,
            ),
        );
        config.custom_providers = vec![
            custom_provider("custom_a", "first-model", ApiFormat::OpenaiCompat),
            custom_provider("最后_β", "  最后模型  ", ApiFormat::Anthropic),
        ];
        assert_endpoint(
            endpoint_for_config(&config),
            (
                "最后_β",
                "https://gateway.example/v1/messages",
                "  最后模型  ",
                30,
                ApiFormat::Anthropic,
            ),
        );
    }
}

#[test]
fn disabled_roles_ignore_saved_provider_and_model_overrides() {
    for role in ROLES {
        let mut config = LlmProviderConfig {
            active: "custom".to_string(),
            custom_base_url: Some("https://legacy.example/full-endpoint/#".to_string()),
            custom_model: Some(" active-model ".to_string()),
            ..Default::default()
        };
        role.configure(&mut config, false, Some("openai"), Some("stale-role-model"));
        let endpoint = role.resolve(&config);
        if matches!(role, Role::ScreenVision) {
            assert!(endpoint.is_none());
        } else {
            assert_endpoint(
                endpoint.expect("active endpoint"),
                (
                    "custom",
                    "https://legacy.example/full-endpoint",
                    "active-model",
                    10,
                    ApiFormat::OpenaiCompat,
                ),
            );
        }
    }
}

#[test]
fn each_role_overrides_the_model_of_the_same_preset_and_preserves_its_url() {
    for role in ROLES {
        for &(provider, default_url, _, timeout) in PRESETS {
            let mut config = LlmProviderConfig {
                active: provider.to_string(),
                custom_model: Some("active-model".to_string()),
                custom_base_url: Some("https://legacy.example/v1/responses/".to_string()),
                ..Default::default()
            };
            role.configure(&mut config, true, Some(provider), Some(" \t角色/模型 β\n"));
            let url = if provider == "custom" {
                "https://legacy.example/v1/responses"
            } else {
                default_url
            };
            assert_endpoint(
                role.resolve(&config).expect("role endpoint"),
                (
                    provider,
                    url,
                    "角色/模型 β",
                    timeout,
                    ApiFormat::OpenaiCompat,
                ),
            );
        }
    }
}

#[test]
fn assistant_and_validation_keep_active_preset_fields_when_the_role_model_is_blank() {
    for role in [Role::Assistant, Role::Validation] {
        for &(provider, default_url, _, timeout) in PRESETS {
            for role_provider in [Some(provider), None, Some("removed-role-provider")] {
                for role_model in [None, Some(""), Some(" \t\n")] {
                    let mut config = LlmProviderConfig {
                        active: provider.to_string(),
                        custom_model: Some(" \t当前模型/β\n".to_string()),
                        custom_base_url: Some(" https://legacy.example/v1/responses/ ".to_string()),
                        ..Default::default()
                    };
                    role.configure(&mut config, true, role_provider, role_model);
                    let url = if provider == "custom" {
                        "https://legacy.example/v1/responses"
                    } else {
                        default_url
                    };
                    assert_endpoint(
                        role.resolve(&config).expect("active preset endpoint"),
                        (
                            provider,
                            url,
                            "当前模型/β",
                            timeout,
                            ApiFormat::OpenaiCompat,
                        ),
                    );
                }
            }
        }
    }
}

#[test]
fn each_role_switching_to_a_preset_does_not_leak_active_legacy_fields() {
    for role in ROLES {
        for &(provider, url, _, timeout) in PRESETS {
            let mut config = LlmProviderConfig {
                active: if provider == "deepseek" {
                    "openai"
                } else {
                    "deepseek"
                }
                .to_string(),
                custom_model: Some("stale-active-model".to_string()),
                custom_base_url: Some("https://stale.example/v1".to_string()),
                ..Default::default()
            };
            role.configure(&mut config, true, Some(provider), Some("role-model"));
            assert_endpoint(
                role.resolve(&config).expect("role endpoint"),
                (
                    provider,
                    url,
                    "role-model",
                    timeout,
                    ApiFormat::OpenaiCompat,
                ),
            );
        }
    }
}

#[test]
fn assistant_and_validation_can_switch_presets_without_an_explicit_model() {
    for role in [Role::Assistant, Role::Validation] {
        for &(provider, url, model, timeout) in PRESETS {
            for role_model in [None, Some(" \t")] {
                let mut config = LlmProviderConfig {
                    active: "custom_a".to_string(),
                    custom_model: Some("stale-preset-model".to_string()),
                    custom_base_url: Some("https://stale.example/v1".to_string()),
                    custom_providers: vec![custom_provider(
                        "custom_a",
                        "active-model",
                        ApiFormat::Anthropic,
                    )],
                    ..Default::default()
                };
                role.configure(&mut config, true, Some(provider), role_model);
                assert_endpoint(
                    role.resolve(&config).expect("role endpoint"),
                    (provider, url, model, timeout, ApiFormat::OpenaiCompat),
                );
            }
        }
    }
}

#[test]
fn each_role_can_override_the_same_or_a_different_custom_entry() {
    for role in ROLES {
        for same_provider in [true, false] {
            for (format, url, timeout) in [
                (
                    ApiFormat::OpenaiCompat,
                    "https://target.example/v1/chat/completions",
                    10,
                ),
                (
                    ApiFormat::Anthropic,
                    "https://target.example/v1/messages",
                    30,
                ),
            ] {
                let mut target = custom_provider("供应商_β", "  原始模型  ", format.clone());
                target.base_url = " https://target.example/v1/ ".to_string();
                let mut config = LlmProviderConfig {
                    active: if same_provider {
                        "供应商_β"
                    } else {
                        "custom_a"
                    }
                    .to_string(),
                    custom_model: Some("stale-preset-model".to_string()),
                    custom_base_url: Some("https://stale.example".to_string()),
                    custom_providers: vec![
                        custom_provider("custom_a", "active-model", ApiFormat::OpenaiCompat),
                        target,
                    ],
                    ..Default::default()
                };
                role.configure(&mut config, true, Some("供应商_β"), Some("  角色模型/β \t"));
                assert_endpoint(
                    role.resolve(&config).expect("custom role endpoint"),
                    ("供应商_β", url, "角色模型/β", timeout, format),
                );
                assert_eq!(config.custom_providers[1].model, "  原始模型  ");
                assert_endpoint(
                    endpoint_for_config(&config),
                    if same_provider {
                        (
                            "供应商_β",
                            url,
                            "  原始模型  ",
                            timeout,
                            config.custom_providers[1].api_format.clone(),
                        )
                    } else {
                        (
                            "custom_a",
                            "https://gateway.example/v1/chat/completions",
                            "active-model",
                            10,
                            ApiFormat::OpenaiCompat,
                        )
                    },
                );
            }
        }
    }
}

#[test]
fn assistant_and_validation_use_the_custom_entry_model_when_override_is_blank() {
    for role in [Role::Assistant, Role::Validation] {
        for active in ["custom_a", "openai", "custom_b"] {
            for role_model in [None, Some(""), Some(" \t\n")] {
                let mut other_custom =
                    custom_provider("custom_b", "other-custom-model", ApiFormat::OpenaiCompat);
                other_custom.base_url = "https://active.example/v1".to_string();
                let mut config = LlmProviderConfig {
                    active: active.to_string(),
                    custom_model: Some("stale-preset-model".to_string()),
                    custom_providers: vec![
                        custom_provider("custom_a", "  原始模型/β  ", ApiFormat::Anthropic),
                        other_custom,
                    ],
                    ..Default::default()
                };
                role.configure(&mut config, true, Some("custom_a"), role_model);
                assert_endpoint(
                    role.resolve(&config).expect("custom role endpoint"),
                    (
                        "custom_a",
                        "https://gateway.example/v1/messages",
                        "  原始模型/β  ",
                        30,
                        ApiFormat::Anthropic,
                    ),
                );
                if active == "custom_b" {
                    assert_endpoint(
                        endpoint_for_config(&config),
                        (
                            "custom_b",
                            "https://active.example/v1/chat/completions",
                            "other-custom-model",
                            10,
                            ApiFormat::OpenaiCompat,
                        ),
                    );
                }
            }
        }
    }
}

#[test]
fn assistant_and_validation_keep_model_overrides_when_the_provider_is_invalid() {
    for role in [Role::Assistant, Role::Validation] {
        for provider in [None, Some(""), Some("missing"), Some(" openai ")] {
            let mut config = LlmProviderConfig {
                active: "deepseek".to_string(),
                custom_model: Some("active-model".to_string()),
                ..Default::default()
            };
            role.configure(&mut config, true, provider, Some("  角色模型/β  "));
            assert_endpoint(
                role.resolve(&config)
                    .expect("active endpoint with role model"),
                (
                    "deepseek",
                    "https://api.deepseek.com/v1/chat/completions",
                    "角色模型/β",
                    10,
                    ApiFormat::OpenaiCompat,
                ),
            );
        }
    }
}

#[test]
fn incomplete_selection_configuration_falls_back_atomically() {
    for (enabled, provider, model) in [
        (false, Some("openai"), Some("stale-role-model")),
        (true, None, Some("stale-role-model")),
        (true, Some(""), Some("stale-role-model")),
        (true, Some(" \t"), Some("stale-role-model")),
        (true, Some("missing"), Some("stale-role-model")),
        (true, Some("openai"), None),
        (true, Some("openai"), Some("")),
        (true, Some("openai"), Some(" \t\n")),
    ] {
        let mut config = LlmProviderConfig {
            active: "custom_a".to_string(),
            custom_model: Some("stale-preset-model".to_string()),
            custom_providers: vec![custom_provider(
                "custom_a",
                "  原始模型/β  ",
                ApiFormat::Anthropic,
            )],
            ..Default::default()
        };
        Role::Selection.configure(&mut config, enabled, provider, model);
        assert_endpoint(
            selection_endpoint_for_config(&config),
            (
                "custom_a",
                "https://gateway.example/v1/messages",
                "  原始模型/β  ",
                30,
                ApiFormat::Anthropic,
            ),
        );
    }
}

#[test]
fn incomplete_screen_vision_configuration_has_no_endpoint() {
    for (enabled, provider, model) in [
        (false, Some("openai"), Some("vision-model")),
        (true, None, Some("vision-model")),
        (true, Some(""), Some("vision-model")),
        (true, Some(" \t"), Some("vision-model")),
        (true, Some("missing"), Some("vision-model")),
        (true, Some("openai"), None),
        (true, Some("openai"), Some("")),
        (true, Some("openai"), Some(" \t\n")),
    ] {
        let mut config = LlmProviderConfig::default();
        Role::ScreenVision.configure(&mut config, enabled, provider, model);
        assert!(screen_vision_endpoint_for_config(&config).is_none());
    }
}

#[test]
fn selection_and_screen_vision_trim_complete_provider_and_model_overrides() {
    for role in [Role::Selection, Role::ScreenVision] {
        let mut config = LlmProviderConfig {
            active: "deepseek".to_string(),
            custom_model: Some("active-model".to_string()),
            ..Default::default()
        };
        role.configure(
            &mut config,
            true,
            Some(" \topenai\n"),
            Some("  视觉模型/β  "),
        );
        assert_endpoint(
            role.resolve(&config).expect("trimmed role endpoint"),
            (
                "openai",
                "https://api.openai.com/v1/responses",
                "视觉模型/β",
                10,
                ApiFormat::OpenaiCompat,
            ),
        );
    }
}

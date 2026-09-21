use std::collections::HashSet;
use std::future::Future;
use std::time::Duration;

use serde_json::{json, Value};
use tauri::Emitter;
use tokio::sync::Semaphore;

use super::{jev_service, jev_tasks};
use crate::state::user_profile::{CorrectionSource, UserProfile};

const REVIEW_TIMEOUT: Duration = Duration::from_secs(5);
const CORRECTION_BATCH_SIZE: usize = 40;
// Conservative initial cutoffs; model probabilities are not calibrated guarantees.
const CORRECTION_REJECTION_MIN_PROBABILITY: f64 = 0.95;
const AUDIT_WARNING_MIN_PROBABILITY: f64 = 0.90;
static AUDIT_SLOT: Semaphore = Semaphore::const_new(1);

fn spawn_audit_job(job: impl Future<Output = ()> + Send + 'static) -> bool {
    let Ok(permit) = AUDIT_SLOT.try_acquire() else {
        return false;
    };
    tauri::async_runtime::spawn(async move {
        let _permit = permit;
        job.await;
    });
    true
}

pub fn schedule_polish_audit(
    app: &tauri::AppHandle,
    state: &crate::state::AppState,
    session_id: u64,
    original: &str,
    polished: &str,
    policy: String,
) {
    let config = state.with_profile(|profile| profile.jev.clone());
    if !config.polish_audit || original == polished {
        return;
    }
    let app = app.clone();
    let client = state.http_client.clone();
    let evidence = json!({"original":original,"polished":polished,"polishing_policy":policy});
    let admitted = spawn_audit_job(async move {
        let key = jev_service::load_api_key_for_provider(&app, config.provider).unwrap_or_default();
        let result = jev_tasks::evaluate(
            &client,
            config.provider,
            &key,
            evidence,
            jev_tasks::audit_questions(),
            REVIEW_TIMEOUT,
            None,
        )
        .await;
        if let Some(report) = result
            .as_ref()
            .and_then(|response| audit_report(session_id, response))
        {
            log::info!(
                "Jev polish audit: session={}, status={}",
                session_id,
                report["status"]
            );
            let _ = app.emit("jev-polish-audit", report);
        } else {
            log::debug!("Jev polish audit unavailable: session={}", session_id);
        }
    });
    if !admitted {
        log::debug!("Jev polish audit skipped: busy, session={}", session_id);
    }
}

fn invalid_corrections(
    response: &Value,
    rules: &[(String, String)],
) -> Option<HashSet<(String, String)>> {
    let options = ["valid", "invalid", "uncertain"];
    let mut invalid = HashSet::new();
    for (index, rule) in rules.iter().enumerate() {
        let id = format!("rule_{index}");
        // Invalid response structure is a service failure, not an uncertain decision.
        jev_tasks::confident_choice(response, &id, &options, 0.0)?;
        if jev_tasks::confident_choice(
            response,
            &id,
            &options,
            CORRECTION_REJECTION_MIN_PROBABILITY,
        )
        .as_deref()
            == Some("invalid")
        {
            invalid.insert(rule.clone());
        }
    }
    Some(invalid)
}

fn audit_report(session_id: u64, response: &Value) -> Option<Value> {
    let mut changed = Vec::new();
    let mut uncertain = false;
    let options = ["preserved", "changed", "uncertain"];
    for id in ["negation", "numbers", "entities", "intent"] {
        jev_tasks::confident_choice(response, id, &options, 0.0)?;
        match jev_tasks::confident_choice(response, id, &options, AUDIT_WARNING_MIN_PROBABILITY)
            .as_deref()
        {
            Some("changed") => changed.push(id),
            Some("preserved") => {}
            _ => uncertain = true,
        }
    }
    let status = if !changed.is_empty() {
        "possible_change"
    } else if uncertain {
        "uncertain"
    } else {
        "preserved"
    };
    Some(json!({"sessionId":session_id,"status":status,"checks":changed}))
}

pub async fn review_corrections(
    client: &reqwest::Client,
    provider: jev_service::JevProvider,
    key: &str,
    rules: &[(String, String)],
    endpoint_override: Option<&str>,
) -> Option<HashSet<(String, String)>> {
    let mut invalid = HashSet::new();
    for batch in rules.chunks(CORRECTION_BATCH_SIZE) {
        let response = jev_tasks::evaluate(client, provider, key,
            json!({"rules":batch.iter().map(|(original,corrected)| json!({"original":original,"corrected":corrected})).collect::<Vec<_>>(),
                "evidence_limit":"Only learned word pairs are available, not audio or original sentences. Context-dependent substitutions are uncertain."}),
            jev_tasks::correction_questions(batch.len()), REVIEW_TIMEOUT, endpoint_override).await?;
        invalid.extend(invalid_corrections(&response, batch)?);
    }
    Some(invalid)
}

pub fn remove_invalid_ai_rules(
    profile: &mut UserProfile,
    invalid: &HashSet<(String, String)>,
) -> u32 {
    let before = profile.correction_patterns.len();
    profile.correction_patterns.retain(|rule| {
        rule.source != CorrectionSource::Ai
            || !invalid.contains(&(rule.original.clone(), rule.corrected.clone()))
    });
    (before - profile.correction_patterns.len()) as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn applying_review_preserves_user_rules_and_reports_actual_removals() {
        use crate::state::user_profile::CorrectionPattern;
        let mut profile = UserProfile::default();
        for source in [CorrectionSource::Ai, CorrectionSource::User] {
            profile.correction_patterns.push(CorrectionPattern {
                original: "same".into(),
                corrected: "replacement".into(),
                source,
                count: 1,
                last_seen: 1,
            });
        }
        let invalid = HashSet::from([
            ("same".into(), "replacement".into()),
            ("stale".into(), "rule".into()),
        ]);
        assert_eq!(remove_invalid_ai_rules(&mut profile, &invalid), 1);
        assert_eq!(profile.correction_patterns.len(), 1);
        assert_eq!(
            profile.correction_patterns[0].source,
            CorrectionSource::User
        );
        assert_eq!(remove_invalid_ai_rules(&mut profile, &invalid), 0);
    }

    #[tokio::test]
    async fn audit_admission_does_not_wait_and_drops_work_while_busy() {
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel();
        assert!(spawn_audit_job(async move {
            started_tx.send(()).unwrap();
            let _ = release_rx.await;
        }));
        tokio::time::timeout(Duration::from_secs(2), started_rx)
            .await
            .unwrap()
            .unwrap();
        assert!(!spawn_audit_job(async {
            panic!("busy audit must never run")
        }));
        release_tx.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            while AUDIT_SLOT.available_permits() == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn correction_review_batches_rules_without_requiring_an_llm() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            for expected_size in [40, 1] {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut data = Vec::new();
                let mut buffer = [0u8; 4096];
                let header_end = loop {
                    let count = stream.read(&mut buffer).await.unwrap();
                    assert!(count > 0);
                    data.extend_from_slice(&buffer[..count]);
                    if let Some(index) = data.windows(4).position(|p| p == b"\r\n\r\n") {
                        break index + 4;
                    }
                };
                let headers = std::str::from_utf8(&data[..header_end]).unwrap();
                let length: usize = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse().unwrap())
                    })
                    .unwrap();
                while data.len() < header_end + length {
                    let count = stream.read(&mut buffer).await.unwrap();
                    assert!(count > 0);
                    data.extend_from_slice(&buffer[..count]);
                }
                let request: Value =
                    serde_json::from_slice(&data[header_end..header_end + length]).unwrap();
                assert_eq!(
                    request["state"]["rules"].as_array().unwrap().len(),
                    expected_size
                );
                assert_eq!(
                    request["questions"].as_object().unwrap().len(),
                    expected_size
                );
                let options = ["valid", "invalid", "uncertain"];
                let answers = (0..expected_size)
                    .map(|i| {
                        (
                            format!("rule_{i}"),
                            answer(
                                if i == 0 && expected_size == 40 {
                                    "invalid"
                                } else {
                                    "uncertain"
                                },
                                &options,
                                0.98,
                            ),
                        )
                    })
                    .collect::<serde_json::Map<_, _>>();
                let body = json!({"answers":answers}).to_string();
                stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(),body).as_bytes()).await.unwrap();
            }
        });
        let rules = (0..41)
            .map(|i| (format!("original{i}"), format!("corrected{i}")))
            .collect::<Vec<_>>();
        let invalid = review_corrections(
            &reqwest::Client::new(),
            jev_service::JevProvider::Vercel,
            "test-key",
            &rules,
            Some(&endpoint),
        )
        .await
        .unwrap();
        assert_eq!(invalid, HashSet::from([rules[0].clone()]));
        tokio::time::timeout(Duration::from_secs(2), server)
            .await
            .unwrap()
            .unwrap();
    }

    fn answer(choice: &str, choices: &[&str], probability: f64) -> Value {
        let probabilities = choices
            .iter()
            .map(|key| {
                (
                    (*key).to_string(),
                    json!(if *key == choice {
                        probability
                    } else {
                        (1.0 - probability) / (choices.len() - 1) as f64
                    }),
                )
            })
            .collect::<serde_json::Map<_, _>>();
        json!({"type":"choice", "choice":choice, "probabilities":probabilities})
    }

    #[test]
    fn deletes_only_confident_invalid_rules_and_rejects_incomplete_batches() {
        let rules = (0..4)
            .map(|i| (format!("original{i}"), format!("corrected{i}")))
            .collect::<Vec<_>>();
        let options = ["valid", "invalid", "uncertain"];
        let mut response = json!({"answers": {
            "rule_0":answer("invalid", &options, 0.95),
            "rule_1":answer("invalid", &options, 0.94),
            "rule_2":answer("uncertain", &options, 0.98),
            "rule_3":answer("valid", &options, 0.99),
        }});
        assert_eq!(
            invalid_corrections(&response, &rules).unwrap(),
            HashSet::from([rules[0].clone()])
        );
        response["answers"]
            .as_object_mut()
            .unwrap()
            .remove("rule_2");
        assert!(invalid_corrections(&response, &rules).is_none());
    }

    #[test]
    fn audit_is_conservative_and_event_contains_no_transcript() {
        let options = ["preserved", "changed", "uncertain"];
        let mut answers = serde_json::Map::new();
        for id in ["negation", "numbers", "entities", "intent"] {
            answers.insert(id.into(), answer("preserved", &options, 0.98));
        }
        let mut response = json!({"answers":answers, "text":"private transcript"});
        assert_eq!(audit_report(31, &response).unwrap()["status"], "preserved");
        response["answers"]["negation"] = answer("changed", &options, 0.96);
        let warning = audit_report(31, &response).unwrap();
        assert_eq!(
            warning,
            json!({"sessionId":31,"status":"possible_change","checks":["negation"]})
        );
        response["answers"]["negation"] = answer("changed", &options, 0.65);
        assert_eq!(audit_report(31, &response).unwrap()["status"], "uncertain");
        response["answers"]
            .as_object_mut()
            .unwrap()
            .remove("intent");
        assert!(audit_report(31, &response).is_none());
    }
}

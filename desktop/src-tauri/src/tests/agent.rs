#[test]
fn agent_harness_builds_an_expert_prompt_from_history_and_tool_evidence() {
    let preview = agent_harness::prompt_preview(
        &json!({
            "message": "比较自选股里的主线机会",
            "mode": "expert",
            "history": [
                {"role": "user", "content": "先看市场环境"},
                {"role": "assistant", "content": "需要结合本地证据"}
            ]
        }),
        &json!({
            "reply": "已完成本地趋势筛选。",
            "action": "trend_screen",
            "evidence_summary": [{"title": "趋势筛选", "source": "本地K线", "level": "primary"}]
        }),
    );

    assert_eq!(preview["profile_id"], "hot_money_early_v1");
    assert!(preview["system_prompt"]
        .as_str()
        .unwrap_or_default()
        .contains("仅供选股研究"));
    assert!(preview["system_prompt"]
        .as_str()
        .unwrap_or_default()
        .contains("情绪周期"));
    assert_eq!(
        preview["user_payload"]["history"].as_array().map(Vec::len),
        Some(2)
    );
    assert_eq!(
        preview["user_payload"]["tool_result"]["action"],
        "trend_screen"
    );
    assert_eq!(preview["user_payload"]["tool_result_truncated"], false);
}

#[test]
fn agent_harness_marks_compacted_tool_context() {
    let preview = agent_harness::prompt_preview(
        &json!({"message": "比较候选", "mode": "expert"}),
        &json!({"items": (0..20).map(|index| json!({"index": index})).collect::<Vec<_>>() }),
    );
    assert_eq!(preview["user_payload"]["tool_result_truncated"], true);
    assert_eq!(
        preview["user_payload"]["tool_result"]["items"]
            .as_array()
            .map(Vec::len),
        Some(16)
    );
}

#[test]
fn agent_harness_builds_a_value_compounder_prompt_for_research_mode() {
    let preview = agent_harness::prompt_preview(
        &json!({
            "message": "从长期持有角度研究这家公司",
            "mode": "research"
        }),
        &json!({
            "reply": "已读取公司财务与公告证据。",
            "action": "observe_stock",
            "evidence_summary": [{"title": "年度报告", "source": "交易所公告", "level": "primary"}]
        }),
    );

    let prompt = preview["system_prompt"].as_str().unwrap_or_default();
    assert_eq!(preview["profile_id"], "value_compounder_v1");
    for required in [
        "格雷厄姆",
        "费雪",
        "巴菲特",
        "芒格",
        "护城河",
        "所有者收益",
        "资本配置",
        "安全边际",
    ] {
        assert!(
            prompt.contains(required),
            "research prompt should contain {required}"
        );
    }
    assert!(!prompt.contains("打板"));
    assert!(!prompt.contains("二板"));
    assert!(prompt.contains("不构成投资建议"));
}

#[test]
fn agent_harness_prompt_profiles_pass_contrast_evaluation() {
    let suite: Value = serde_json::from_str(include_str!(
        "../../../../app/prompts/agent_harness_eval_cases.json"
    ))
    .expect("agent harness evaluation fixture should be valid JSON");
    let tool_result = json!({
        "reply": "本地工具已完成取证。",
        "action": "observe_stock",
        "data": {"code": "000001.SZ", "returned": 1},
        "evidence_summary": [{"title": "财报与行情", "source": "本地工具", "level": "primary"}]
    });

    for case in suite["cases"].as_array().expect("evaluation cases") {
        let question = case["question"].as_str().expect("case question");
        for mode in ["expert", "research"] {
            let contract = &suite["profiles"][mode];
            let preview = agent_harness::prompt_preview(
                &json!({"message": question, "mode": mode}),
                &tool_result,
            );
            let prompt = preview["system_prompt"].as_str().unwrap_or_default();
            assert_eq!(preview["profile_id"], contract["profile_id"]);
            assert_eq!(preview["user_payload"]["question"], question);
            for term in contract["required_prompt_terms"]
                .as_array()
                .expect("required terms")
            {
                let term = term.as_str().unwrap();
                assert!(prompt.contains(term), "{mode} prompt should contain {term}");
            }
            for term in contract["forbidden_prompt_terms"]
                .as_array()
                .expect("forbidden terms")
            {
                let term = term.as_str().unwrap();
                assert!(
                    !prompt.contains(term),
                    "{mode} prompt should not contain {term}"
                );
            }
            let merged = agent_harness::merge_model_response(
                tool_result.clone(),
                &case[format!("{mode}_output")],
                contract["profile_id"].as_str().unwrap(),
                Some("eval-model"),
            )
            .expect("safe evaluation sample should pass output gates");
            assert_eq!(merged["action"], tool_result["action"]);
            assert_eq!(merged["data"], tool_result["data"]);
            let rendered = merged.to_string();
            for term in case[format!("{mode}_required_output_terms")]
                .as_array()
                .expect("required output terms")
            {
                let term = term.as_str().unwrap();
                assert!(
                    rendered.contains(term),
                    "{mode} output should contain {term}"
                );
            }
            for term in contract["forbidden_prompt_terms"]
                .as_array()
                .expect("cross-profile terms")
            {
                let term = term.as_str().unwrap();
                assert!(
                    !rendered.contains(term),
                    "{mode} output should not contain {term}"
                );
            }
        }
    }

    for unsafe_output in suite["unsafe_outputs"].as_array().expect("unsafe outputs") {
        for profile_id in ["hot_money_early_v1", "value_compounder_v1"] {
            assert!(agent_harness::merge_model_response(
                tool_result.clone(),
                unsafe_output,
                profile_id,
                Some("eval-model"),
            )
            .is_err());
        }
    }
}

#[test]
fn agent_harness_merges_model_explanation_without_overwriting_tool_facts() {
    let tool_response = json!({
        "reply": "本地工具结论",
        "action": "trend_screen",
        "data": {"returned": 3, "items": [{"code": "000001.SZ"}]},
        "evidence_summary": [{"title": "趋势筛选", "source": "本地K线", "level": "primary", "summary": "返回 3 个候选"}],
        "answer_sections": [{"title": "本地工具事实", "bullets": ["返回 3 个候选。"]}],
        "warnings": ["工具提示：部分行情数据缺失。"],
        "next_actions": ["复核本地行情"]
    });
    let merged = agent_harness::merge_model_response(
        tool_response.clone(),
        &json!({
            "reply": "模型基于本地证据完成解释。[E1]",
            "action": "backtest",
            "data": {"returned": 999},
            "answer_sections": [{"title": "市场环境", "bullets": ["当前证据偏强，但仍需复核。[E1]"]}],
            "warnings": ["模型结论存在不确定性。"],
            "next_actions": ["核验公告原文"]
        }),
        "hot_money_early_v1",
        Some("test-model"),
    )
    .expect("safe model output should merge");

    assert_eq!(merged["action"], tool_response["action"]);
    assert_eq!(merged["data"], tool_response["data"]);
    assert!(merged["reply"].as_str().unwrap().contains("本地工具结论"));
    assert!(merged["reply"]
        .as_str()
        .unwrap()
        .contains("模型基于本地证据完成解释"));
    assert!(merged["answer_sections"]
        .as_array()
        .unwrap()
        .iter()
        .any(|section| { section["title"].as_str() == Some("本地工具事实") }));
    assert!(merged["model_answer_sections"]
        .as_array()
        .unwrap()
        .iter()
        .any(|section| {
            section["title"].as_str() == Some("市场环境")
                && section["provenance"].as_str() == Some("model_inference")
        }));
    assert!(merged["next_actions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|action| { action.as_str() == Some("复核本地行情") }));
    assert!(merged["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item.as_str() == Some("仅供选股研究，不构成投资建议。")));
    assert!(merged["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item
            .as_str()
            .unwrap_or_default()
            .contains("部分行情数据缺失")));
    assert_eq!(merged["harness"]["model_used"], true);
    assert_eq!(merged["harness"]["profile_id"], "hot_money_early_v1");
}

#[test]
fn agent_harness_rejects_direct_trading_or_manipulation_in_model_output() {
    for unsafe_reply in [
        "建议立即买入并满仓持有。",
        "可以通过虚假申报和拉抬股价吸引跟风盘。",
    ] {
        let result = agent_harness::merge_model_response(
            json!({"reply": "工具结论", "action": "screen", "data": {"returned": 1}}),
            &json!({"reply": unsafe_reply}),
            "hot_money_early_v1",
            Some("test-model"),
        );
        assert!(
            result.is_err(),
            "unsafe output should be rejected: {unsafe_reply}"
        );
    }
}

#[test]
fn agent_harness_rejects_missing_or_unknown_model_evidence_references() {
    let tool_response = json!({
        "reply": "工具结论",
        "action": "screen",
        "data": {"returned": 1},
        "evidence_summary": [{"title": "筛选结果", "source": "本地股票池", "level": "primary", "summary": "返回 1 项"}]
    });
    for reply in ["模型结论没有引用。", "模型错误引用。[E2]"] {
        let result = agent_harness::merge_model_response(
            tool_response.clone(),
            &json!({"reply": reply}),
            "hot_money_early_v1",
            Some("test-model"),
        );
        assert!(
            result.is_err(),
            "invalid evidence should be rejected: {reply}"
        );
    }
}

#[test]
fn agent_harness_calls_an_openai_compatible_model_endpoint() {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock model endpoint");
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept model request");
        let mut request = vec![0_u8; 65_536];
        let read = stream.read(&mut request).expect("read model request");
        let request = String::from_utf8_lossy(&request[..read]);
        assert!(request.starts_with("POST /v1/chat/completions "));
        assert!(request.contains("hot_money_early_v1"));

        let content = json!({
            "reply": "模型链路已接通。",
            "answer_sections": [{"title": "市场环境", "bullets": ["依据本地证据解释。"]}],
            "warnings": ["仅供选股研究，不构成投资建议。"],
            "next_actions": ["核验原始数据"]
        })
        .to_string();
        let body = json!({"choices": [{"message": {"content": content}}]}).to_string();
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body,
        ).expect("write model response");
    });

    let preview = agent_harness::prompt_preview(
        &json!({"message": "分析市场主线", "mode": "expert"}),
        &json!({"reply": "工具结果", "action": "trend_screen", "data": {"returned": 2}}),
    );
    let result = tauri::async_runtime::block_on(agent_harness::call_model(
        Some(&json!({
            "base_url": format!("http://{address}/v1"),
            "model": "mock-model",
            "timeout_seconds": 5,
            "json_mode": true
        })),
        &preview,
    ))
    .expect("model request should succeed")
    .expect("model should be configured");

    assert_eq!(result["reply"], "模型链路已接通。");
    server.join().expect("mock model server should finish");
}

#[test]
fn agent_harness_treats_incomplete_model_settings_as_unconfigured() {
    let preview = agent_harness::prompt_preview(
        &json!({"message": "分析市场主线", "mode": "expert"}),
        &json!({"reply": "工具结果", "action": "trend_screen"}),
    );
    let result = tauri::async_runtime::block_on(agent_harness::call_model(
        Some(&json!({"temperature": 0.7, "timeout_seconds": 1})),
        &preview,
    ))
    .expect("incomplete settings should not trigger a request");
    assert!(result.is_none());
}

#[test]
fn agent_harness_only_retries_when_an_endpoint_rejects_json_mode() {
    assert!(agent_harness::should_retry_without_json_mode(
        "Agent LLM HTTP 400 Bad Request: response_format json_object is unsupported"
    ));
    for error in [
        "Agent LLM HTTP 401 Unauthorized",
        "Agent LLM HTTP 500 Internal Server Error",
        "Agent LLM request failed: timed out",
        "parse Agent model JSON failed",
    ] {
        assert!(!agent_harness::should_retry_without_json_mode(error));
    }
}

#[test]
fn agent_harness_rejects_invalid_model_endpoints_before_network_io() {
    let preview = agent_harness::prompt_preview(
        &json!({"message": "分析市场主线", "mode": "expert"}),
        &json!({"reply": "工具结果", "action": "trend_screen"}),
    );
    let error = tauri::async_runtime::block_on(agent_harness::call_model(
        Some(&json!({"base_url": "file:///tmp/model", "model": "mock"})),
        &preview,
    ))
    .expect_err("non-HTTP model endpoints must be rejected");
    assert!(error.contains("http or https"));
    let error = tauri::async_runtime::block_on(agent_harness::call_model(
        Some(&json!({"base_url": "http://models.example.test/v1", "model": "mock"})),
        &preview,
    ))
    .expect_err("remote plaintext model endpoints must be rejected");
    assert!(error.contains("must use https"));
}

#[test]
fn agent_harness_executes_local_tools_then_synthesizes_the_final_result() {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};
    use std::thread;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock model endpoint");
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept model request");
        let mut request = vec![0_u8; 65_536];
        let _ = stream.read(&mut request).expect("read model request");
        let content = json!({
            "reply": "已按专家模式解释本地自选股。[E1]",
            "answer_sections": [{"title": "市场环境", "bullets": ["当前只有自选股事实，情绪位置待验证。[E1]"]}],
            "warnings": ["仅供选股研究，不构成投资建议。"],
            "next_actions": ["补充行情与公告证据"]
        }).to_string();
        let body = json!({"choices": [{"message": {"content": content}}]}).to_string();
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body,
        ).unwrap();
    });

    let streamed = Arc::new(Mutex::new(Vec::new()));
    let streamed_sink = Arc::clone(&streamed);
    let outcome = tauri::async_runtime::block_on(agent_harness::execute_with_event_sink(
        json!({
            "message": "查看自选股",
            "run_id": "harness-run",
            "mode": "expert",
            "llm": {
                "base_url": format!("http://{address}/v1"),
                "model": "mock-model",
                "timeout_seconds": 5
            },
            "context": {"watchlist": [{"code": "000001.SZ", "name": "平安银行"}]}
        }),
        json!({}),
        move |event| streamed_sink.lock().unwrap().push(event),
    ))
    .expect("harness should execute");

    assert_eq!(outcome.response["action"], "watchlist_action");
    assert_eq!(outcome.response["harness"]["model_used"], true);
    assert_eq!(
        outcome.response["harness"]["profile_id"],
        "hot_money_early_v1"
    );
    assert!(outcome.events.iter().any(|event| event["type"] == "result"));
    let streamed = streamed.lock().unwrap();
    assert_eq!(
        streamed.first().and_then(|event| event["stage"].as_str()),
        Some("tools")
    );
    let model_index = streamed
        .iter()
        .position(|event| event["stage"] == "model")
        .unwrap();
    let result_index = streamed
        .iter()
        .position(|event| event["type"] == "result")
        .unwrap();
    assert!(model_index < result_index);
    server.join().unwrap();
}

#[test]
fn agent_harness_quick_mode_reports_configured_model_failure() {
    let outcome = tauri::async_runtime::block_on(agent_harness::execute(
        json!({
            "message": "查看自选股",
            "run_id": "quick-run",
            "mode": "quick",
            "llm": {
                "base_url": "http://127.0.0.1:9/v1",
                "model": "configured-model",
                "timeout_seconds": 1
            },
            "context": {"watchlist": [{"code": "000001.SZ", "name": "平安银行"}]}
        }),
        json!({}),
    ))
    .expect("quick harness should preserve the model failure outcome");

    assert_eq!(outcome.response["harness"]["profile_id"], "quick_agent_v1");
    assert_eq!(outcome.response["harness"]["model_used"], false);
    assert_eq!(
        outcome.response["harness"]["model_outcome"],
        "request_failed"
    );
}

#[cfg(target_os = "windows")]
#[test]
fn research_embedding_job_gate_replays_requests_arriving_during_a_run() {
    let gate = ResearchEmbeddingJobGate::new();
    assert!(gate.request());
    gate.begin_cycle();
    assert!(!gate.request());
    assert!(gate.finish_cycle());
    gate.begin_cycle();
    assert!(!gate.finish_cycle());
    assert!(gate.request());
}

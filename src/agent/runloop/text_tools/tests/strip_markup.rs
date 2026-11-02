use super::*;

#[test]
fn test_strip_textual_tool_call_regions_removes_tools_call_markup() {
    let message = r#"Researching the launch flow.
<tools:call name="code_search">
<parameter name="pattern">fn main</parameter>
</tools:call>
Now drafting the plan."#;

    let stripped = strip_textual_tool_call_regions(message);

    assert!(stripped.contains("Researching the launch flow."));
    assert!(stripped.contains("Now drafting the plan."));
    assert!(!stripped.contains("<tools:call"));
    assert!(detect_textual_tool_call(&stripped).is_none());
}
#[test]
fn test_strip_textual_tool_call_regions_removes_minimax_markup() {
    let message = r#"Done with the analysis.
<minimax:tool_call>
<invoke name="read_file">
<parameter name="action">read</parameter>
<parameter name="path">README.md</parameter>
</invoke>
</minimax:tool_call>
Please review the summary."#;

    let stripped = strip_textual_tool_call_regions(message);

    assert!(stripped.contains("Done with the analysis."));
    assert!(stripped.contains("Please review the summary."));
    assert!(!stripped.contains("<invoke"));
    assert!(detect_textual_tool_call(&stripped).is_none());
}
#[test]
fn test_strip_textual_tool_call_regions_removes_spaced_dsml_markup() {
    let message = concat!(
        "Researching the launch flow.\n",
        "<\u{ff5c} \u{ff5c} DSML\u{ff5c} \u{ff5c} invoke name=\"exec_command\">\n",
        "<\u{ff5c} \u{ff5c} DSML\u{ff5c} \u{ff5c} parameter name=\"cmd\" string=\"true\">true</\u{ff5c} \u{ff5c} DSML\u{ff5c} \u{ff5c} parameter>\n",
        "</\u{ff5c} \u{ff5c} DSML\u{ff5c} \u{ff5c} invoke>\n",
        "Now drafting the plan.",
    );

    let stripped = strip_textual_tool_call_regions(message);

    assert!(stripped.contains("Researching the launch flow."));
    assert!(stripped.contains("Now drafting the plan."));
    assert!(!stripped.contains("DSML"));
}
#[test]
fn test_detect_minimax_child_element_parameters() {
    let message = r#"
<minimax:tool_call>
<invoke name="exec_command">
<action>run</action>
<command>python3 -c "print('hello')"</command>
</invoke>
</minimax:tool_call>
"#;
    let (name, args) = detect_textual_tool_call(message).expect("should parse child-element format");
    assert_eq!(name, tools::EXEC_COMMAND);
    assert_eq!(
        args,
        serde_json::json!({
            "action": "run",
            "command": ["python3", "-c", "print('hello')"]
        })
    );
}
#[test]
fn test_detect_minimax_noisy_format() {
    let message = r#"]<]minimax[>[<tool_call>
]<]minimax[>[<invoke name="code_search">]<]minimax[>[<query>Widget</query>]<]minimax[>[<path>src</path>]<]minimax[>[<result_types>["definition"]</result_types>]<]minimax[>[</invoke>
]<]minimax[>[</tool_call>"#;
    let (name, args) = detect_textual_tool_call(message).expect("should parse noisy format");
    assert_eq!(name, tools::CODE_SEARCH);
    assert_eq!(
        args,
        serde_json::json!({
            "query": "Widget",
            "path": "src",
            "result_types": ["definition"]
        })
    );
}
#[test]
fn test_detect_minimax_parameter_name_format_still_works() {
    // Regression test: ensure the old <parameter name="..."> format still works
    let message = r#"
<minimax:tool_call>
<invoke name="read_file">
<parameter name="action">read</parameter>
<parameter name="path">README.md</parameter>
</invoke>
</minimax:tool_call>
"#;
    let (name, args) = detect_textual_tool_call(message).expect("should parse parameter name format");
    assert_eq!(name, tools::READ_FILE);
    assert_eq!(
        args,
        serde_json::json!({
            "action": "read",
            "path": "README.md"
        })
    );
}
#[test]
fn test_strip_textual_tool_call_regions_removes_channel_and_function_markup() {
    let message = concat!(
        "Summary before.\n",
        "<|start|>assistant<|channel|>commentary to=bash<|message|>{\"cmd\":\"pwd\"}<|call|>\n",
        "Middle.\n",
        "print(default_api.read_file(path='AGENTS.md'))\n",
        "Summary after."
    );

    let stripped = strip_textual_tool_call_regions(message);

    assert!(stripped.contains("Summary before."));
    assert!(stripped.contains("Middle."));
    assert!(stripped.contains("Summary after."));
    assert!(!stripped.contains("<|start|>"));
    assert!(!stripped.contains("print("));
    assert!(!stripped.contains("default_api.read_file"));
    assert!(detect_textual_tool_call(&stripped).is_none());
}

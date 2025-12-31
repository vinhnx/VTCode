1: use serde::{Deserialize, Serialize};
2: use std::collections::HashMap;
3: use std::fs;
4: use std::path::Path;
5: 
6: #[derive(Debug, Clone, Serialize, Deserialize)]
7: pub struct OutputStyleConfig {
8:     #[serde(default = "default_output_style")]
9:     pub active_style: String,
10: }
11: 
12: fn default_output_style() -> String {
13:     "default".to_string()
14: }
15: 
16: impl Default for OutputStyleManager {
17:     fn default() -> Self {
18:         Self {
19:             active_style: default_output_style(),
20:         }
21:     }
22: }
23: 
24: #[derive(Debug, Clone, Serialize, Deserialize)]
25: #[serde(rename_all = "kebab-case")]
26: pub struct OutputStyleFileConfig {
27:     pub name: String,
28:     pub description: Option<String>,
29:     #[serde(default)]
30:     pub keep_coding_instructions: bool,
… [+199 lines omitted; use read_file with offset/limit for full content]
230:         let base_prompt = "Base system prompt";
231:         let result = manager.apply_style("Test Style", base_prompt);
232: 
233:         assert!(result.contains("Base system prompt"));
234:         assert!(result.contains("Custom instructions here"));
235:     }
236: 
237:     #[test]
238:     fn test_apply_style_without_keep_instructions() {
239:         let content = r#"---
240: name: Test Style
241: description: A test output style
242: keep-coding-instructions: false
243: ---
244: 
245: ## Custom Instructions
246: 
247: Custom instructions here."#;
248: 
249:         let style = OutputStyleManager::parse_output_style(content).unwrap();
250:         let mut manager = OutputStyleManager::new();
251:         manager.styles.insert("Test Style".to_string(), style);
252: 
253:         let base_prompt = "Base system prompt";
254:         let result = manager.apply_style("Test Style", base_prompt);
255: 
256:         assert!(!result.contains("Base system prompt"));
257:         assert!(result.contains("Custom instructions here"));
258:     }
259: }
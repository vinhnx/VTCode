//! Language-specific functionality and queries for tree-sitter

use crate::tools::tree_sitter::analyzer::{LanguageSupport, Position, SyntaxNode};
use serde::{Deserialize, Serialize};

/// Language-specific queries and operations
pub struct LanguageQueries {
    pub functions_query: String,
    pub classes_query: String,
    pub imports_query: String,
    pub variables_query: String,
    pub comments_query: String,
}

impl LanguageQueries {
    /// Get queries for a specific language
    pub fn for_language(language: &LanguageSupport) -> Self {
        match language {
            LanguageSupport::Rust => Self::rust_queries(),
            LanguageSupport::Python => Self::python_queries(),
            LanguageSupport::JavaScript => Self::javascript_queries(),
            LanguageSupport::TypeScript => Self::typescript_queries(),
            LanguageSupport::Go => Self::go_queries(),
            LanguageSupport::Java => Self::java_queries(),
            LanguageSupport::Bash => Self::bash_queries(),
            LanguageSupport::Swift => Self::swift_queries(),
        }
    }

    fn rust_queries() -> Self {
        Self {
            functions_query: r#"
                (function_item
                    name: (identifier) @function.name
                    parameters: (parameters) @function.parameters
                    return_type: (return_type)? @function.return_type
                    body: (block) @function.body) @function.def

                (impl_item
                    type: (type_identifier) @impl.type
                    body: (declaration_list) @impl.body)

                (trait_item
                    name: (type_identifier) @trait.name
                    body: (declaration_list) @trait.body)
            "#
            .to_string(),

            classes_query: r#"
                (struct_item
                    name: (type_identifier) @struct.name
                    body: (field_declaration_list) @struct.fields) @struct.def

                (enum_item
                    name: (type_identifier) @enum.name
                    body: (enum_variant_list) @enum.variants) @enum.def
            "#
            .to_string(),

            imports_query: r#"
                (use_declaration
                    argument: (scoped_identifier) @import.path) @import.def

                (mod_item
                    name: (identifier) @module.name) @module.def
            "#
            .to_string(),

            variables_query: r#"
                (let_declaration
                    pattern: (identifier) @variable.name
                    type: (type_annotation)? @variable.type
                    value: (expression)? @variable.value) @variable.def

                (const_item
                    name: (identifier) @const.name
                    type: (type_annotation)? @const.type
                    value: (expression) @const.value) @const.def

                (static_item
                    name: (identifier) @static.name
                    type: (type_annotation)? @static.type
                    value: (expression)? @static.value) @static.def
            "#
            .to_string(),

            comments_query: r#"
                (line_comment) @comment.line
                (block_comment) @comment.block
            "#
            .to_string(),
        }
    }

    fn python_queries() -> Self {
        Self {
            functions_query: r#"
                (function_definition
                    name: (identifier) @function.name
                    parameters: (parameters) @function.parameters
                    body: (block) @function.body) @function.def

                (class_definition
                    name: (identifier) @class.name
                    body: (block) @class.body) @class.def
            "#
            .to_string(),

            classes_query: r#"
                (class_definition
                    name: (identifier) @class.name
                    superclasses: (argument_list)? @class.superclasses
                    body: (block) @class.body) @class.def
            "#
            .to_string(),

            imports_query: r#"
                (import_statement
                    name: (dotted_name) @import.name) @import.def

                (import_from_statement
                    module: (dotted_name) @import.module
                    name: (dotted_name) @import.name) @import.def
            "#
            .to_string(),

            variables_query: r#"
                (assignment
                    left: (identifier) @variable.name
                    right: (expression) @variable.value) @variable.def
            "#
            .to_string(),

            comments_query: r#"
                (comment) @comment
            "#
            .to_string(),
        }
    }

    fn javascript_queries() -> Self {
        Self {
            functions_query: r#"
                (function_declaration
                    name: (identifier) @function.name
                    parameters: (formal_parameters) @function.parameters
                    body: (statement_block) @function.body) @function.def

                (function_expression
                    name: (identifier)? @function.name
                    parameters: (formal_parameters) @function.parameters
                    body: (statement_block) @function.body) @function.expr

                (arrow_function
                    parameters: (formal_parameters) @arrow.parameters
                    body: (statement_block) @arrow.body) @arrow.def
            "#
            .to_string(),

            classes_query: r#"
                (class_declaration
                    name: (identifier) @class.name
                    body: (class_body) @class.body) @class.def
            "#
            .to_string(),

            imports_query: r#"
                (import_statement
                    source: (string) @import.source
                    specifiers: (import_clause) @import.specifiers) @import.def

                (export_statement
                    declaration: (function_declaration) @export.function) @export.def
            "#
            .to_string(),

            variables_query: r#"
                (variable_declaration
                    declarator: (variable_declarator
                        name: (identifier) @variable.name
                        value: (expression)? @variable.value)) @variable.def

                (lexical_declaration
                    declarator: (variable_declarator
                        name: (identifier) @variable.name
                        value: (expression)? @variable.value)) @variable.def
            "#
            .to_string(),

            comments_query: r#"
                (comment) @comment
            "#
            .to_string(),
        }
    }

    fn typescript_queries() -> Self {
        // TypeScript extends JavaScript queries
        let mut js_queries = Self::javascript_queries();

        // Add TypeScript-specific queries
        js_queries.functions_query.push_str(
            r#"
            (interface_declaration
                name: (type_identifier) @interface.name
                body: (interface_body) @interface.body) @interface.def

            (type_alias_declaration
                name: (type_identifier) @type.name
                value: (type_annotation) @type.value) @type.def
        "#,
        );

        js_queries.classes_query.push_str(
            r#"
            (interface_declaration
                name: (type_identifier) @interface.name
                body: (interface_body) @interface.body) @interface.def
        "#,
        );

        js_queries
    }

    fn go_queries() -> Self {
        Self {
            functions_query: r#"
                (function_declaration
                    name: (identifier) @function.name
                    parameters: (parameter_list) @function.parameters
                    result: (parameter_list)? @function.result
                    body: (block) @function.body) @function.def

                (method_declaration
                    receiver: (parameter_list) @method.receiver
                    name: (identifier) @method.name
                    parameters: (parameter_list) @method.parameters
                    result: (parameter_list)? @method.result
                    body: (block) @method.body) @method.def
            "#
            .to_string(),

            classes_query: r#"
                (type_declaration
                    name: (type_identifier) @type.name
                    type_spec: (type_spec
                        name: (type_identifier) @struct.name
                        type: (struct_type) @struct.def)) @type.def

                (interface_type
                    method_spec: (method_spec) @interface.method) @interface.def
            "#
            .to_string(),

            imports_query: r#"
                (import_declaration
                    spec: (import_spec
                        path: (interpreted_string_literal) @import.path
                        name: (identifier)? @import.name)) @import.def
            "#
            .to_string(),

            variables_query: r#"
                (var_declaration
                    spec: (var_spec
                        name: (identifier) @variable.name
                        type: (type_identifier)? @variable.type
                        value: (expression_list)? @variable.value)) @variable.def

                (short_var_declaration
                    left: (expression_list) @var.names
                    right: (expression_list) @var.values) @short.var
            "#
            .to_string(),

            comments_query: r#"
                (comment) @comment
            "#
            .to_string(),
        }
    }

    fn java_queries() -> Self {
        Self {
            functions_query: r#"
                (method_declaration
                    name: (identifier) @method.name
                    parameters: (formal_parameters) @method.parameters
                    type: (type_identifier)? @method.return_type
                    body: (block) @method.body) @method.def

                (constructor_declaration
                    name: (identifier) @constructor.name
                    parameters: (formal_parameters) @constructor.parameters
                    body: (block) @constructor.body) @constructor.def
            "#
            .to_string(),

            classes_query: r#"
                (class_declaration
                    name: (identifier) @class.name
                    body: (class_body) @class.body) @class.def

                (interface_declaration
                    name: (identifier) @interface.name
                    body: (interface_body) @interface.body) @interface.def
            "#
            .to_string(),

            imports_query: r#"
                (import_declaration
                    qualified_name: (qualified_name) @import.name) @import.def

                (package_declaration
                    qualified_name: (qualified_name) @package.name) @package.def
            "#
            .to_string(),

            variables_query: r#"
                (field_declaration
                    declarator: (variable_declarator
                        name: (identifier) @field.name
                        dimensions: (dimensions)? @field.dimensions)
                    type: (type_identifier) @field.type) @field.def

                (local_variable_declaration
                    declarator: (variable_declarator
                        name: (identifier) @variable.name)
                    type: (type_identifier) @variable.type) @variable.def
            "#
            .to_string(),

            comments_query: r#"
                (line_comment) @comment.line
                (block_comment) @comment.block
            "#
            .to_string(),
        }
    }

    fn bash_queries() -> Self {
        Self {
            functions_query: r#"
                (function_definition
                    name: (word) @function.name
                    body: (compound_statement) @function.body) @function.def

                (function_definition
                    name: (name) @function.name
                    body: (compound_statement) @function.body) @function.def
            "#
            .to_string(),

            classes_query: String::new(),

            imports_query: r#"
                (command
                    name: (command_name
                        (word) @import.name)
                    argument: (_)? @import.path
                ) @import.def
                (#match? @import.name "^(source|\.)$")
            "#
            .to_string(),

            variables_query: r#"
                (variable_assignment
                    name: (word) @variable.name
                    value: (_)? @variable.value) @variable.def
            "#
            .to_string(),

            comments_query: r#"
                (comment) @comment.line
            "#
            .to_string(),
        }
    }

    #[allow(dead_code)]
    fn swift_queries() -> Self {
        Self {
            functions_query: r#"
                (function_declaration
                    name: (simple_identifier) @function.name
                    parameter: (parameter_clause) @function.parameters
                    return_type: (type_annotation)? @function.return_type
                    body: (function_body) @function.body) @function.def

                (method_declaration
                    name: (simple_identifier) @method.name
                    parameter: (parameter_clause) @method.parameters
                    return_type: (type_annotation)? @method.return_type
                    body: (function_body) @method.body) @method.def

                (initializer_declaration
                    parameter: (parameter_clause) @initializer.parameters
                    body: (function_body) @initializer.body) @initializer.def

                (deinitializer_declaration
                    body: (function_body) @deinitializer.body) @deinitializer.def
            "#
            .to_string(),

            classes_query: r#"
                (class_declaration
                    name: (type_identifier) @class.name
                    inheritance: (inheritance_clause)? @class.inheritance
                    body: (class_body) @class.body) @class.def

                (struct_declaration
                    name: (type_identifier) @struct.name
                    inheritance: (inheritance_clause)? @struct.inheritance
                    body: (struct_body) @struct.body) @struct.def

                (enum_declaration
                    name: (type_identifier) @enum.name
                    inheritance: (inheritance_clause)? @enum.inheritance
                    body: (enum_body) @enum.body) @enum.def

                (protocol_declaration
                    name: (type_identifier) @protocol.name
                    inheritance: (inheritance_clause)? @protocol.inheritance
                    body: (protocol_body) @protocol.body) @protocol.def

                (extension_declaration
                    extended_type: (type_identifier) @extension.type
                    inheritance: (inheritance_clause)? @extension.inheritance
                    body: (extension_body) @extension.body) @extension.def
            "#
            .to_string(),

            imports_query: r#"
                (import_declaration
                    import_kind: (import_kind)? @import.kind
                    path: (import_path) @import.path) @import.def
            "#
            .to_string(),

            variables_query: r#"
                (property_declaration
                    name: (pattern) @property.name
                    type_annotation: (type_annotation)? @property.type
                    initializer: (initializer_clause)? @property.initializer) @property.def

                (constant_declaration
                    name: (pattern) @constant.name
                    type_annotation: (type_annotation)? @constant.type
                    initializer: (initializer_clause) @constant.initializer) @constant.def

                (variable_declaration
                    name: (pattern) @variable.name
                    type_annotation: (type_annotation)? @variable.type
                    initializer: (initializer_clause)? @variable.initializer) @variable.def

                (parameter
                    name: (simple_identifier) @parameter.name
                    type_annotation: (type_annotation) @parameter.type) @parameter.def
            "#
            .to_string(),

            comments_query: r#"
                (comment) @comment
                (multiline_comment) @comment.multiline
            "#
            .to_string(),
        }
    }
}

/// Symbol information extracted from code
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolInfo {
    pub name: String,
    pub kind: SymbolKind,
    pub position: Position,
    pub scope: Option<String>,
    pub signature: Option<String>,
    pub documentation: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SymbolKind {
    Function,
    Method,
    Class,
    Struct,
    Interface,
    Trait,
    Variable,
    Constant,
    Import,
    Module,
    Type,
}

/// Language-specific symbol extraction
pub struct LanguageAnalyzer {
    #[allow(dead_code)]
    queries: LanguageQueries,
}

impl LanguageAnalyzer {
    pub fn new(language: &LanguageSupport) -> Self {
        Self {
            queries: LanguageQueries::for_language(language),
        }
    }

    /// Extract symbols from syntax tree
    pub fn extract_symbols(
        &self,
        tree: &crate::tools::tree_sitter::analyzer::SyntaxTree,
    ) -> Vec<SymbolInfo> {
        let mut symbols = Vec::new();

        // Extract functions
        symbols.extend(self.extract_functions(&tree.root));

        // Extract classes/structs
        symbols.extend(self.extract_classes(&tree.root));

        // Extract variables
        symbols.extend(self.extract_variables(&tree.root));

        // Extract imports
        symbols.extend(self.extract_imports(&tree.root));

        symbols
    }

    fn extract_functions(&self, node: &SyntaxNode) -> Vec<SymbolInfo> {
        let mut functions = Vec::new();

        if node.kind.contains("function") || node.kind.contains("method") {
            if let Some(name_node) = node
                .named_children
                .get("name")
                .and_then(|children| children.first())
                .or_else(|| {
                    node.named_children
                        .get("identifier")
                        .and_then(|children| children.first())
                })
                .or_else(|| {
                    node.named_children
                        .get("word")
                        .and_then(|children| children.first())
                })
            {
                let function = SymbolInfo {
                    name: name_node.text.clone(),
                    kind: if node.kind.contains("method") {
                        SymbolKind::Method
                    } else {
                        SymbolKind::Function
                    },
                    position: name_node.start_position.clone(),
                    scope: Some(
                        if node.kind.contains("method") {
                            "method"
                        } else {
                            "function"
                        }
                        .to_string(),
                    ),
                    signature: self.extract_signature(node),
                    documentation: self.extract_documentation(node),
                };
                functions.push(function);
            }
        }

        // Recursively extract from children
        for child in &node.children {
            functions.extend(self.extract_functions(child));
        }

        functions
    }

    fn extract_classes(&self, node: &SyntaxNode) -> Vec<SymbolInfo> {
        let mut classes = Vec::new();

        if node.kind.contains("class")
            || node.kind.contains("struct")
            || node.kind.contains("interface")
        {
            if let Some(name_node) = node
                .named_children
                .get("name")
                .and_then(|children| children.first())
            {
                let kind = match node.kind.as_str() {
                    k if k.contains("interface") => SymbolKind::Interface,
                    k if k.contains("struct") => SymbolKind::Struct,
                    _ => SymbolKind::Class,
                };

                let class = SymbolInfo {
                    name: name_node.text.clone(),
                    kind,
                    position: name_node.start_position.clone(),
                    scope: Some("class".to_string()),
                    signature: None,
                    documentation: self.extract_documentation(node),
                };
                classes.push(class);
            }
        }

        // Recursively extract from children
        for child in &node.children {
            classes.extend(self.extract_classes(child));
        }

        classes
    }

    fn extract_variables(&self, node: &SyntaxNode) -> Vec<SymbolInfo> {
        let mut variables = Vec::new();

        if node.kind.contains("variable")
            || node.kind.contains("const")
            || node.kind.contains("let")
            || node.kind.contains("assignment")
        {
            // Extract variable names from children
            for child in &node.children {
                if (child.kind == "identifier" || child.kind == "word") && !child.text.is_empty() {
                    let variable = SymbolInfo {
                        name: child.text.clone(),
                        kind: if node.kind.contains("const") {
                            SymbolKind::Constant
                        } else {
                            SymbolKind::Variable
                        },
                        position: child.start_position.clone(),
                        scope: Some("variable".to_string()),
                        signature: None,
                        documentation: None,
                    };
                    variables.push(variable);
                    break; // Only take the first identifier (variable name)
                }
            }
        }

        // Recursively extract from children
        for child in &node.children {
            variables.extend(self.extract_variables(child));
        }

        variables
    }

    fn extract_imports(&self, node: &SyntaxNode) -> Vec<SymbolInfo> {
        let mut imports = Vec::new();

        if node.kind.contains("import") {
            // Extract import information
            for child in &node.children {
                if child.kind.contains("identifier") || child.kind.contains("name") {
                    let import = SymbolInfo {
                        name: child.text.clone(),
                        kind: SymbolKind::Import,
                        position: child.start_position.clone(),
                        scope: Some("import".to_string()),
                        signature: None,
                        documentation: None,
                    };
                    imports.push(import);
                }
            }
        }

        // Recursively extract from children
        for child in &node.children {
            imports.extend(self.extract_imports(child));
        }

        imports
    }

    fn extract_signature(&self, node: &SyntaxNode) -> Option<String> {
        // Extract function/method signature
        if let Some(params_node) = node
            .named_children
            .get("parameters")
            .and_then(|children| children.first())
        {
            let params = &params_node.text;

            let return_type = node
                .named_children
                .get("return_type")
                .and_then(|children| children.first())
                .map(|rt| format!(" -> {}", rt.text))
                .unwrap_or_default();

            Some(format!("({}){}", params, return_type))
        } else {
            None
        }
    }

    fn extract_documentation(&self, node: &SyntaxNode) -> Option<String> {
        // Heuristic: combine leading sibling comments (captured during AST build)
        // with any immediate child comment nodes.
        let mut docs = Vec::new();

        // Preceding sibling comments collected on the node
        for c in &node.leading_comments {
            if !c.is_empty() {
                docs.push(c.clone());
            }
        }

        // Immediate child comments
        for child in &node.children {
            let kind = child.kind.to_lowercase();
            if kind.contains("comment") {
                let t = child.text.trim();
                if !t.is_empty() {
                    docs.push(t.to_string());
                }
            }
        }

        if docs.is_empty() {
            None
        } else {
            Some(docs.join("\n"))
        }
    }
}

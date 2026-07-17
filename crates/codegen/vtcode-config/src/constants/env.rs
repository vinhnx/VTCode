/// Agent Client Protocol specific environment keys
pub mod acp {
    #[derive(Debug, Clone, Copy)]
    pub enum AgentClientProtocolEnvKey {
        Enabled,
        ZedEnabled,
        ZedToolsReadFileEnabled,
        ZedToolsListFilesEnabled,
        ZedWorkspaceTrust,
    }

    impl AgentClientProtocolEnvKey {
        pub fn as_str(self) -> &'static str {
            match self {
                Self::Enabled => "VT_ACP_ENABLED",
                Self::ZedEnabled => "VT_ACP_ZED_ENABLED",
                Self::ZedToolsReadFileEnabled => "VT_ACP_ZED_TOOLS_READ_FILE_ENABLED",
                Self::ZedToolsListFilesEnabled => "VT_ACP_ZED_TOOLS_LIST_FILES_ENABLED",
                Self::ZedWorkspaceTrust => "VT_ACP_ZED_WORKSPACE_TRUST",
            }
        }
    }
}

pub(crate) fn supported_signal_names() -> Vec<&'static str> {
    #[cfg(unix)]
    {
        vec![
            "SIGHUP", "SIGINT", "SIGQUIT", "SIGILL", "SIGTRAP", "SIGABRT", "SIGBUS", "SIGFPE",
            "SIGKILL", "SIGUSR1", "SIGSEGV", "SIGUSR2", "SIGPIPE", "SIGALRM", "SIGTERM",
        ]
    }
    #[cfg(not(unix))]
    {
        // Windows 仅支持少量信号；SIGINT/SIGBREAK/SIGTERM 可由运行时解释。
        vec!["SIGINT", "SIGTERM", "SIGKILL"]
    }
}

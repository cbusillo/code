use code_app_server::AppServerTransport;
use code_app_server::run_main_with_transport;
use code_arg0::arg0_dispatch_or_else;
use code_common::CliConfigOverrides;

fn main() -> anyhow::Result<()> {
    arg0_dispatch_or_else(|code_linux_sandbox_exe| async move {
        let transport = AppServerTransport::from_listen_args(std::env::args());
        run_main_with_transport(
            code_linux_sandbox_exe,
            CliConfigOverrides::default(),
            transport,
        )
        .await?;
        Ok(())
    })
}

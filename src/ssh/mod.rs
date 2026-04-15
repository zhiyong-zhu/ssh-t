use anyhow::Result;
use async_trait::async_trait;
use russh::client;
use std::io::Cursor;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::config::HostConfig;

/// Messages sent from SSH session to the TUI.
#[derive(Debug)]
pub enum SshEvent {
    Connected { id: usize, name: String },
    Output { id: usize, data: Vec<u8> },
    Disconnected { id: usize, reason: String },
    Error { id: usize, message: String },
    SftpReady { id: usize },
}

impl SshEvent {
    pub fn session_id(&self) -> usize {
        match self {
            SshEvent::Connected { id, .. } => *id,
            SshEvent::Output { id, .. } => *id,
            SshEvent::Disconnected { id, .. } => *id,
            SshEvent::Error { id, .. } => *id,
            SshEvent::SftpReady { id, .. } => *id,
        }
    }
}

/// SSH client handler for russh callbacks.
struct ClientHandler;

#[async_trait]
impl client::Handler for ClientHandler {
    type Error = anyhow::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &russh_keys::ssh_key::PublicKey,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

/// Input message for the shell task.
pub enum ShellInput {
    Data(Vec<u8>),
    Resize { cols: u16, rows: u16 },
    Eof,
}

/// Manages an SSH connection lifecycle.
pub struct SshManager {
    session_id: usize,
    config: HostConfig,
    event_tx: mpsc::UnboundedSender<SshEvent>,
    session: Option<client::Handle<ClientHandler>>,
    shell_input_tx: Option<mpsc::UnboundedSender<ShellInput>>,
    /// Override password if provided (for dialog input)
    password_override: Option<String>,
}

impl SshManager {
    pub fn new(session_id: usize, config: HostConfig, event_tx: mpsc::UnboundedSender<SshEvent>) -> Self {
        Self {
            session_id,
            config,
            event_tx,
            session: None,
            shell_input_tx: None,
            password_override: None,
        }
    }

    /// Create a manager with a pre-provided password.
    pub fn with_password(session_id: usize, config: HostConfig, event_tx: mpsc::UnboundedSender<SshEvent>, password: String) -> Self {
        Self {
            session_id,
            config,
            event_tx,
            session: None,
            shell_input_tx: None,
            password_override: Some(password),
        }
    }

    /// Connect to the remote host.
    pub async fn connect(&mut self) -> Result<()> {
        let config = client::Config::default();
        let handler = ClientHandler;
        let mut session =
            client::connect(Arc::new(config), (&*self.config.host, self.config.port), handler)
                .await?;

        let auth_ok = match &self.config.auth {
            crate::config::AuthMethod::Password => {
                // Use override password if provided, otherwise get from keyring
                let password = if let Some(ref pwd) = self.password_override {
                    pwd.clone()
                } else {
                    crate::cred::CredentialStore::get_password(
                        &self.config.host,
                        &self.config.user,
                    )?
                };

                // Try keyboard-interactive first (most servers prefer this)
                let ki_result = session
                    .authenticate_keyboard_interactive_start(&self.config.user, None::<String>)
                    .await?;

                match ki_result {
                    client::KeyboardInteractiveAuthResponse::Success => true,
                    client::KeyboardInteractiveAuthResponse::InfoRequest { prompts, .. } => {
                        // Respond to all prompts with the password
                        let responses: Vec<String> = prompts.iter().map(|_| password.clone()).collect();
                        let reply = session
                            .authenticate_keyboard_interactive_respond(responses)
                            .await?;
                        match reply {
                            client::KeyboardInteractiveAuthResponse::Success => true,
                            client::KeyboardInteractiveAuthResponse::InfoRequest { prompts, .. } => {
                                // Server may send another round of prompts
                                let responses: Vec<String> = prompts.iter().map(|_| password.clone()).collect();
                                let reply2 = session
                                    .authenticate_keyboard_interactive_respond(responses)
                                    .await?;
                                matches!(reply2, client::KeyboardInteractiveAuthResponse::Success)
                            }
                            client::KeyboardInteractiveAuthResponse::Failure => {
                                session
                                    .authenticate_password(&self.config.user, &password)
                                    .await?
                            }
                        }
                    }
                    client::KeyboardInteractiveAuthResponse::Failure => {
                        session
                            .authenticate_password(&self.config.user, &password)
                            .await?
                    }
                }
            }
            crate::config::AuthMethod::Key { key_path } => {
                let key = russh_keys::load_secret_key(key_path, None)?;
                let key_with_alg =
                    russh_keys::key::PrivateKeyWithHashAlg::new(Arc::new(key), None)?;
                session
                    .authenticate_publickey(&self.config.user, key_with_alg)
                    .await?
            }
            crate::config::AuthMethod::Agent => {
                let mut agent = russh_keys::agent::client::AgentClient::connect_env().await?;
                let identities = agent.request_identities().await?;
                if identities.is_empty() {
                    anyhow::bail!("No identities found in SSH agent");
                }
                let pub_key = identities[0].clone();
                session
                    .authenticate_publickey_with(&self.config.user, pub_key, &mut agent)
                    .await?
            }
        };

        if !auth_ok {
            anyhow::bail!("Authentication failed for {}", self.config.host);
        }

        // Note: Connected event is sent from the spawn task after open_shell succeeds
        self.session = Some(session);
        Ok(())
    }

    /// Open an interactive PTY shell.
    pub async fn open_shell(&mut self, cols: u16, rows: u16) -> Result<()> {
        let session = self
            .session
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Not connected"))?;

        let channel = session.channel_open_session().await?;

        // Request PTY
        channel
            .request_pty(
                true,
                "xterm-256color",
                cols as u32,
                rows as u32,
                0,
                0,
                &[],
            )
            .await?;

        // Request shell
        channel.request_shell(true).await?;

        // Create input channel
        let (input_tx, mut input_rx) = mpsc::unbounded_channel::<ShellInput>();
        self.shell_input_tx = Some(input_tx);

        // Clone event sender for the task
        let event_tx = self.event_tx.clone();
        let session_id = self.session_id;

        // Spawn task to handle input/output
        tokio::spawn(async move {
            let mut ch = channel;

            loop {
                tokio::select! {
                    // Handle input from TUI
                    input = input_rx.recv() => {
                        match input {
                            Some(ShellInput::Data(data)) => {
                                if let Err(e) = ch.data(&mut Cursor::new(&data)).await {
                                    event_tx.send(SshEvent::Error { id: session_id, message: format!("Send error: {e}") }).ok();
                                    break;
                                }
                            }
                            Some(ShellInput::Resize { cols, rows }) => {
                                if let Err(e) = ch.window_change(cols as u32, rows as u32, 0, 0).await {
                                    event_tx.send(SshEvent::Error { id: session_id, message: format!("Resize error: {e}") }).ok();
                                }
                            }
                            Some(ShellInput::Eof) => {
                                let _ = ch.eof().await;
                                break;
                            }
                            None => {
                                break;
                            }
                        }
                    }

                    // Handle output from server
                    msg = ch.wait() => {
                        match msg {
                            Some(russh::ChannelMsg::Data { data }) => {
                                event_tx.send(SshEvent::Output { id: session_id, data: data.to_vec() }).ok();
                            }
                            Some(russh::ChannelMsg::ExtendedData { data, ext: 1 }) => {
                                event_tx.send(SshEvent::Output { id: session_id, data: data.to_vec() }).ok();
                            }
                            Some(russh::ChannelMsg::Eof) => {
                                event_tx.send(SshEvent::Disconnected { id: session_id, reason: "EOF".into() }).ok();
                                break;
                            }
                            Some(russh::ChannelMsg::ExitStatus { exit_status }) => {
                                event_tx.send(SshEvent::Disconnected {
                                    id: session_id,
                                    reason: format!("Process exited with code {exit_status}")
                                }).ok();
                                break;
                            }
                            Some(russh::ChannelMsg::Close) => {
                                event_tx.send(SshEvent::Disconnected { id: session_id, reason: "Channel closed".into() }).ok();
                                break;
                            }
                            None => {
                                event_tx.send(SshEvent::Disconnected { id: session_id, reason: "Channel ended".into() }).ok();
                                break;
                            }
                            _ => {}
                        }
                    }
                }
            }
        });

        Ok(())
    }

    /// Send input to the shell.
    pub fn send_input(&self, data: &[u8]) -> Result<()> {
        if let Some(tx) = &self.shell_input_tx {
            tx.send(ShellInput::Data(data.to_vec()))?;
        }
        Ok(())
    }

    /// Resize the PTY.
    pub fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        if let Some(tx) = &self.shell_input_tx {
            tx.send(ShellInput::Resize { cols, rows })?;
        }
        Ok(())
    }

    /// Execute a single command (non-interactive).
    pub async fn exec(&mut self, command: &str) -> Result<()> {
        let session = self
            .session
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Not connected"))?;

        let channel = session.channel_open_session().await?;
        channel.exec(true, command).await?;

        let event_tx = self.event_tx.clone();
        let session_id = self.session_id;
        tokio::spawn(async move {
            let mut ch = channel;
            while let Some(msg) = ch.wait().await {
                match msg {
                    russh::ChannelMsg::Data { data } => {
                        event_tx.send(SshEvent::Output { id: session_id, data: data.to_vec() }).ok();
                    }
                    russh::ChannelMsg::ExtendedData { data, ext: 1 } => {
                        event_tx.send(SshEvent::Output { id: session_id, data: data.to_vec() }).ok();
                    }
                    russh::ChannelMsg::ExitStatus { exit_status } => {
                        event_tx
                            .send(SshEvent::Output {
                                id: session_id,
                                data: format!("\n[Exit code: {exit_status}]\n").into_bytes(),
                            })
                            .ok();
                        break;
                    }
                    russh::ChannelMsg::Eof => {
                        break;
                    }
                    _ => {}
                }
            }
        });

        Ok(())
    }

    /// Open a subsystem channel (for SFTP).
    pub async fn open_sftp_stream(&mut self) -> Result<russh::ChannelStream<client::Msg>> {
        let session = self
            .session
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Not connected"))?;

        let channel = session.channel_open_session().await?;
        channel.request_subsystem(true, "sftp").await?;
        Ok(channel.into_stream())
    }

    /// Disconnect the session.
    pub async fn disconnect(&mut self) -> Result<()> {
        // Close shell input
        if let Some(tx) = self.shell_input_tx.take() {
            let _ = tx.send(ShellInput::Eof);
        }

        if let Some(session) = self.session.take() {
            session
                .disconnect(russh::Disconnect::ByApplication, "Bye", "en")
                .await?;
        }
        Ok(())
    }

    /// Get the host name.
    pub fn host_name(&self) -> &str {
        &self.config.name
    }

    /// Check if connected.
    pub fn is_connected(&self) -> bool {
        self.session.is_some()
    }

    /// Get a clone of the host config.
    pub fn config(&self) -> HostConfig {
        self.config.clone()
    }

    /// Get the session id.
    pub fn session_id(&self) -> usize {
        self.session_id
    }
}

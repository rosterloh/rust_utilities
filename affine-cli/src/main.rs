mod client;
mod config;
mod queries;
mod sync;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};
use reqwest::Method;
use serde_json::{Map, Value, json};

use client::{AffineClient, AuthState};
use config::{ConfigFile, default_config_path};

const DEFAULT_CLIENT_VERSION: &str = "0.25.0";
/// Below this threshold `blob upload` uses the original single-shot GraphQL mutation.
/// Above it, we ask the server for an upload plan (presigned PUT or multipart).
const LARGE_BLOB_BYTES: u64 = 5 * 1024 * 1024;

#[derive(Parser, Debug)]
#[command(name = "affine-cli", version, about = "CLI for the AFFiNE GraphQL API")]
struct Cli {
    #[arg(long, env = "AFFINE_BASE_URL")]
    server: Option<String>,
    #[arg(long, env = "AFFINE_API_TOKEN")]
    token: Option<String>,
    #[arg(long)]
    config: Option<PathBuf>,
    #[arg(long, env = "AFFINE_CLIENT_VERSION", default_value = DEFAULT_CLIENT_VERSION)]
    client_version: String,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    #[command(subcommand)]
    Auth(AuthCommand),
    Graphql(GraphqlCommand),
    #[command(subcommand)]
    Workspace(WorkspaceCommand),
    #[command(subcommand)]
    Doc(DocCommand),
    #[command(subcommand)]
    Blob(BlobCommand),
    #[command(subcommand)]
    Config(ConfigCommand),
}

#[derive(Subcommand, Debug)]
enum AuthCommand {
    Login(LoginArgs),
    Logout,
    Whoami,
    /// Retrieve the current session record from /api/auth/session.
    Session,
    /// List all concurrent sessions for the signed-in user.
    Sessions,
    /// Exchange a magic-link OTP for a session.
    MagicLinkConfirm(MagicLinkConfirmArgs),
    /// Trigger an e-mail verification flow.
    #[command(subcommand)]
    VerifyEmail(VerifyEmailCommand),
    /// Build an OAuth authorization URL for a given provider (google, github, ...).
    Oauth(OauthArgs),
    /// Manage personal access tokens.
    #[command(subcommand)]
    Token(TokenCommand),
}

#[derive(Args, Debug)]
struct LoginArgs {
    email: String,
    #[arg(long)]
    password: Option<String>,
    /// Send a magic-link e-mail instead of using a password.
    #[arg(long)]
    magic_link: bool,
    #[arg(long)]
    callback_url: Option<String>,
}

#[derive(Args, Debug)]
struct MagicLinkConfirmArgs {
    email: String,
    token: String,
    #[arg(long)]
    callback_url: Option<String>,
}

#[derive(Subcommand, Debug)]
enum VerifyEmailCommand {
    /// Send a verification e-mail that links back to `--callback-url`.
    Send {
        #[arg(long)]
        callback_url: String,
    },
    /// Confirm a verification token received via e-mail.
    Confirm { token: String },
}

#[derive(Args, Debug)]
struct OauthArgs {
    provider: String,
    /// Where the provider should redirect after approval.
    #[arg(long)]
    callback_url: Option<String>,
}

#[derive(Subcommand, Debug)]
enum TokenCommand {
    /// Generate a new personal access token.
    Create {
        #[arg(long)]
        name: String,
        /// ISO-8601 expiry (e.g. `2027-01-01T00:00:00Z`).
        #[arg(long)]
        expires_at: Option<String>,
    },
    /// List tokens belonging to the current user (metadata only).
    List,
    /// Revoke a token by its id.
    Revoke { id: String },
}

#[derive(Args, Debug)]
struct GraphqlCommand {
    #[arg(long, conflicts_with = "query_file")]
    query: Option<String>,
    #[arg(long, conflicts_with = "query")]
    query_file: Option<PathBuf>,
    #[arg(long)]
    variables: Option<String>,
    #[arg(long)]
    operation_name: Option<String>,
}

#[derive(Subcommand, Debug)]
enum WorkspaceCommand {
    List,
    Get {
        id: String,
    },
    Create {
        #[arg(long)]
        init: Option<PathBuf>,
    },
    Update(WorkspaceUpdateArgs),
    Delete {
        id: String,
    },
}

#[derive(Args, Debug)]
struct WorkspaceUpdateArgs {
    id: String,
    #[arg(long)]
    public: Option<bool>,
    #[arg(long)]
    enable_ai: Option<bool>,
    #[arg(long)]
    enable_sharing: Option<bool>,
    #[arg(long)]
    enable_url_preview: Option<bool>,
    #[arg(long)]
    enable_doc_embedding: Option<bool>,
}

#[derive(Subcommand, Debug)]
enum DocCommand {
    List(DocListArgs),
    /// Fetch recently updated docs (populates title/summary).
    Recent(DocListArgs),
    /// List docs that have been published publicly.
    PublicList { workspace_id: String },
    Get {
        workspace_id: String,
        doc_id: String,
    },
    Search(DocSearchArgs),
    /// View analytics for a single doc.
    Analytics(DocAnalyticsArgs),
    /// Publish a doc so anyone with the link can view it.
    Publish(DocPublishArgs),
    /// Revoke public access from a doc.
    Unpublish {
        workspace_id: String,
        doc_id: String,
    },
    /// Manage per-user roles on a doc.
    #[command(subcommand)]
    Role(DocRoleCommand),
    /// Move a doc to the workspace trash (via the real-time sync endpoint).
    Trash {
        workspace_id: String,
        doc_id: String,
    },
}

#[derive(Args, Debug)]
struct DocListArgs {
    workspace_id: String,
    #[arg(long, default_value_t = 20)]
    first: i64,
    #[arg(long)]
    after: Option<String>,
    #[arg(long, default_value_t = 0)]
    offset: i64,
    /// When set on `list`, fetch each doc individually so title/summary are populated
    /// (the raw list query returns nulls until the indexer runs).
    #[arg(long)]
    resolve: bool,
}

#[derive(Args, Debug)]
struct DocSearchArgs {
    workspace_id: String,
    keyword: String,
    #[arg(long, default_value_t = 20)]
    limit: i64,
}

#[derive(Args, Debug)]
struct DocAnalyticsArgs {
    workspace_id: String,
    doc_id: String,
    /// Rolling window, in days (server default is 30).
    #[arg(long)]
    window_days: Option<i64>,
    /// IANA timezone name (e.g. `Europe/London`).
    #[arg(long)]
    timezone: Option<String>,
}

#[derive(Args, Debug)]
struct DocPublishArgs {
    workspace_id: String,
    doc_id: String,
    /// Page (document view) or Edgeless (whiteboard view).
    #[arg(long)]
    mode: Option<PublicDocMode>,
}

#[derive(Clone, Debug, ValueEnum)]
#[clap(rename_all = "PascalCase")]
enum PublicDocMode {
    Page,
    Edgeless,
}

impl PublicDocMode {
    fn as_graphql(&self) -> &'static str {
        match self {
            Self::Page => "Page",
            Self::Edgeless => "Edgeless",
        }
    }
}

#[derive(Clone, Debug, ValueEnum)]
#[clap(rename_all = "PascalCase")]
enum DocRole {
    Owner,
    Manager,
    Editor,
    Reader,
    Commenter,
    External,
    None,
}

impl DocRole {
    fn as_graphql(&self) -> &'static str {
        match self {
            Self::Owner => "Owner",
            Self::Manager => "Manager",
            Self::Editor => "Editor",
            Self::Reader => "Reader",
            Self::Commenter => "Commenter",
            Self::External => "External",
            Self::None => "None",
        }
    }
}

#[derive(Subcommand, Debug)]
enum DocRoleCommand {
    /// Grant a role to one or more users.
    Grant(DocRoleGrantArgs),
    /// Change the role of a single user.
    Update(DocRoleUpdateArgs),
    /// Remove a user's access to a doc.
    Revoke(DocRoleRevokeArgs),
    /// Set the default role applied to workspace members who aren't explicitly listed.
    Default(DocRoleDefaultArgs),
}

#[derive(Args, Debug)]
struct DocRoleGrantArgs {
    workspace_id: String,
    doc_id: String,
    /// Repeat for each user id.
    #[arg(long = "user", required = true)]
    users: Vec<String>,
    #[arg(long)]
    role: DocRole,
}

#[derive(Args, Debug)]
struct DocRoleUpdateArgs {
    workspace_id: String,
    doc_id: String,
    #[arg(long = "user")]
    user: String,
    #[arg(long)]
    role: DocRole,
}

#[derive(Args, Debug)]
struct DocRoleRevokeArgs {
    workspace_id: String,
    doc_id: String,
    #[arg(long = "user")]
    user: String,
}

#[derive(Args, Debug)]
struct DocRoleDefaultArgs {
    workspace_id: String,
    doc_id: String,
    #[arg(long)]
    role: DocRole,
}

#[derive(Subcommand, Debug)]
enum BlobCommand {
    List {
        workspace_id: String,
    },
    /// Upload a file. For files smaller than ~5 MiB (or with `--mode graphql`) uses the
    /// single-shot mutation; otherwise asks the server for a presigned plan and streams
    /// directly to object storage.
    Upload(BlobUploadArgs),
    Download {
        workspace_id: String,
        key: String,
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Probe a blob's size / content-type without fetching the body.
    Head {
        workspace_id: String,
        key: String,
    },
    /// Show storage usage for the workspace.
    Usage {
        workspace_id: String,
    },
    /// Permanently remove any blobs that have been soft-deleted.
    Release {
        workspace_id: String,
    },
    /// Abort an in-flight multipart upload.
    AbortUpload {
        workspace_id: String,
        key: String,
        #[arg(long)]
        upload_id: String,
    },
    Delete {
        workspace_id: String,
        key: String,
        #[arg(long)]
        permanently: bool,
    },
}

#[derive(Args, Debug)]
struct BlobUploadArgs {
    workspace_id: String,
    file: PathBuf,
    /// Force a specific upload path. Defaults to `auto` (picks based on size).
    #[arg(long, default_value = "auto")]
    mode: BlobUploadMode,
    /// Override the blob key; defaults to the file name.
    #[arg(long)]
    key: Option<String>,
    /// Override the content-type; defaults to a guess from the file extension.
    #[arg(long)]
    mime: Option<String>,
}

#[derive(Clone, Debug, ValueEnum)]
#[clap(rename_all = "kebab-case")]
enum BlobUploadMode {
    /// Pick based on file size: GraphQL for small, presigned PUT for large.
    Auto,
    /// Always go through the legacy GraphQL multipart mutation.
    Graphql,
    /// Use `createBlobUpload` + presigned PUT (single part).
    Presigned,
    /// Use `createBlobUpload` + per-part presigned PUTs.
    Multipart,
}

#[derive(Subcommand, Debug)]
enum ConfigCommand {
    Show,
    ClearSession,
}

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let cli = Cli::parse();
    let config_path = cli.config.clone().unwrap_or(default_config_path()?);
    let cli_context = CliContext {
        server: cli.server.clone(),
        token: cli.token.clone(),
        client_version: cli.client_version.clone(),
    };
    let mut config = ConfigFile::load(&config_path)?;

    match cli.command {
        Command::Config(command) => handle_config(command, &config_path, &mut config),
        Command::Auth(command) => {
            handle_auth(command, &cli_context, &config_path, &mut config).await
        }
        Command::Graphql(command) => {
            let client = build_client(&cli_context, &config)?;
            handle_graphql(command, &client).await
        }
        Command::Workspace(command) => {
            let client = build_client(&cli_context, &config)?;
            handle_workspace(command, &client).await
        }
        Command::Doc(command) => {
            let client = build_client(&cli_context, &config)?;
            handle_doc(command, &client).await
        }
        Command::Blob(command) => {
            let client = build_client(&cli_context, &config)?;
            handle_blob(command, &client).await
        }
    }
}

#[derive(Debug, Clone)]
struct CliContext {
    server: Option<String>,
    token: Option<String>,
    client_version: String,
}

fn handle_config(
    command: ConfigCommand,
    config_path: &Path,
    config: &mut ConfigFile,
) -> Result<()> {
    match command {
        ConfigCommand::Show => print_json_pretty(&json!({
            "path": config_path.display().to_string(),
            "config": config
        })),
        ConfigCommand::ClearSession => {
            config.cookies.clear();
            config.save(config_path)?;
            print_json_pretty(&json!({
                "cleared": true,
                "path": config_path.display().to_string(),
            }))
        }
    }
}

async fn handle_auth(
    command: AuthCommand,
    cli: &CliContext,
    config_path: &Path,
    config: &mut ConfigFile,
) -> Result<()> {
    match command {
        AuthCommand::Login(args) => {
            let server = resolve_server(cli.server.as_deref(), config.server.as_deref())?;
            let client =
                AffineClient::new(server.clone(), cli.client_version.clone(), AuthState::None)?;

            let response = if args.magic_link {
                // Send-only: the server e-mails the user an OTP. They must then call
                // `auth magic-link-confirm`.
                client
                    .sign_in(&args.email, None, args.callback_url.as_deref())
                    .await?
            } else if args.password.is_some() {
                client
                    .sign_in(
                        &args.email,
                        args.password.as_deref(),
                        args.callback_url.as_deref(),
                    )
                    .await?
            } else {
                bail!(
                    "pass `--password PW` or `--magic-link` to choose an auth flow"
                );
            };

            config.server = Some(server);
            if !response.cookies.is_empty() {
                config.cookies = response.cookies;
            }
            config.save(config_path)?;

            print_json_pretty(&json!({
                "savedConfig": config_path.display().to_string(),
                "response": response.body,
            }))
        }
        AuthCommand::Logout => {
            let server = resolve_server(cli.server.as_deref(), config.server.as_deref())?;
            let auth = resolved_auth(cli, config);
            let existing_cookies = match &auth {
                AuthState::Cookies(cookies) => cookies.clone(),
                _ => config.cookies.clone(),
            };

            if existing_cookies.is_empty() && !matches!(auth, AuthState::Bearer(_)) {
                bail!("no saved session found; sign in first or pass --token");
            }

            let client = AffineClient::new(server, cli.client_version.clone(), auth)?;
            let cookies = client.sign_out(&existing_cookies).await?;
            config.cookies = cookies;
            config.save(config_path)?;

            print_json_pretty(&json!({
                "signedOut": true,
                "savedConfig": config_path.display().to_string(),
            }))
        }
        AuthCommand::Whoami => {
            let client = build_client(cli, config)?;
            let data = client
                .graphql(queries::CURRENT_USER_QUERY, Some("getCurrentUser"), json!({}))
                .await?;
            print_json_pretty(&data)
        }
        AuthCommand::Session => {
            let client = build_client(cli, config)?;
            let data = client.rest_json(Method::GET, "/api/auth/session", None).await?;
            print_json_pretty(&data)
        }
        AuthCommand::Sessions => {
            let client = build_client(cli, config)?;
            let data = client.rest_json(Method::GET, "/api/auth/sessions", None).await?;
            print_json_pretty(&data)
        }
        AuthCommand::MagicLinkConfirm(args) => {
            let server = resolve_server(cli.server.as_deref(), config.server.as_deref())?;
            let client =
                AffineClient::new(server.clone(), cli.client_version.clone(), AuthState::None)?;
            let response = client
                .magic_link(&args.email, &args.token, args.callback_url.as_deref())
                .await?;

            config.server = Some(server);
            if !response.cookies.is_empty() {
                config.cookies = response.cookies;
            }
            config.save(config_path)?;

            print_json_pretty(&json!({
                "savedConfig": config_path.display().to_string(),
                "response": response.body,
            }))
        }
        AuthCommand::VerifyEmail(sub) => match sub {
            VerifyEmailCommand::Send { callback_url } => {
                let client = build_client(cli, config)?;
                let data = client
                    .graphql(
                        queries::SEND_VERIFY_EMAIL_QUERY,
                        Some("sendVerifyEmail"),
                        json!({ "callbackUrl": callback_url }),
                    )
                    .await?;
                print_json_pretty(&data)
            }
            VerifyEmailCommand::Confirm { token } => {
                let client = build_client(cli, config)?;
                let data = client
                    .graphql(
                        queries::VERIFY_EMAIL_QUERY,
                        Some("verifyEmail"),
                        json!({ "token": token }),
                    )
                    .await?;
                print_json_pretty(&data)
            }
        },
        AuthCommand::Oauth(args) => {
            let server = resolve_server(cli.server.as_deref(), config.server.as_deref())?;
            let mut url = format!(
                "{server}/api/oauth/authorize?provider={provider}",
                provider = urlencoding_encode(&args.provider)
            );
            if let Some(callback_url) = args.callback_url {
                url.push_str("&redirect_uri=");
                url.push_str(&urlencoding_encode(&callback_url));
            }
            print_json_pretty(&json!({
                "authorize_url": url,
                "callback_path": "/api/oauth/callback",
            }))
        }
        AuthCommand::Token(sub) => handle_token_command(sub, cli, config).await,
    }
}

async fn handle_token_command(
    command: TokenCommand,
    cli: &CliContext,
    config: &ConfigFile,
) -> Result<()> {
    let client = build_client(cli, config)?;
    match command {
        TokenCommand::Create { name, expires_at } => {
            let mut input = Map::new();
            input.insert("name".to_owned(), Value::String(name));
            if let Some(expires_at) = expires_at {
                input.insert("expiresAt".to_owned(), Value::String(expires_at));
            }
            let data = client
                .graphql(
                    queries::GENERATE_ACCESS_TOKEN_QUERY,
                    Some("generateUserAccessToken"),
                    json!({ "input": input }),
                )
                .await?;
            print_json_pretty(&data)
        }
        TokenCommand::List => {
            let data = client
                .graphql(
                    queries::LIST_ACCESS_TOKENS_QUERY,
                    Some("listAccessTokens"),
                    json!({}),
                )
                .await?;
            print_json_pretty(&data)
        }
        TokenCommand::Revoke { id } => {
            let data = client
                .graphql(
                    queries::REVOKE_ACCESS_TOKEN_QUERY,
                    Some("revokeUserAccessToken"),
                    json!({ "id": id }),
                )
                .await?;
            print_json_pretty(&data)
        }
    }
}

async fn handle_graphql(command: GraphqlCommand, client: &AffineClient) -> Result<()> {
    let query = match (command.query, command.query_file) {
        (Some(query), None) => query,
        (None, Some(path)) => tokio::fs::read_to_string(&path)
            .await
            .with_context(|| format!("failed to read query file {}", path.display()))?,
        _ => bail!("pass exactly one of --query or --query-file"),
    };

    let variables = parse_optional_json_object(command.variables.as_deref())?;
    let data = client
        .graphql(
            &query,
            command.operation_name.as_deref(),
            Value::Object(variables),
        )
        .await?;
    print_json_pretty(&data)
}

async fn handle_workspace(command: WorkspaceCommand, client: &AffineClient) -> Result<()> {
    match command {
        WorkspaceCommand::List => {
            let data = client
                .graphql(queries::LIST_WORKSPACES_QUERY, Some("getWorkspaces"), json!({}))
                .await?;
            print_json_pretty(&data)
        }
        WorkspaceCommand::Get { id } => {
            let data = client
                .graphql(
                    queries::GET_WORKSPACE_QUERY,
                    Some("getWorkspace"),
                    json!({ "id": id }),
                )
                .await?;
            print_json_pretty(&data)
        }
        WorkspaceCommand::Create { init } => {
            let data = if let Some(init) = init {
                client
                    .graphql_upload(
                        queries::CREATE_WORKSPACE_WITH_INIT_QUERY,
                        "createWorkspace",
                        json!({}),
                        "init",
                        &init,
                    )
                    .await?
            } else {
                client
                    .graphql(queries::CREATE_WORKSPACE_QUERY, Some("createWorkspace"), json!({}))
                    .await?
            };
            print_json_pretty(&data)
        }
        WorkspaceCommand::Update(args) => {
            let mut input = Map::new();
            input.insert("id".to_owned(), Value::String(args.id));

            maybe_insert_bool(&mut input, "public", args.public);
            maybe_insert_bool(&mut input, "enableAi", args.enable_ai);
            maybe_insert_bool(&mut input, "enableSharing", args.enable_sharing);
            maybe_insert_bool(&mut input, "enableUrlPreview", args.enable_url_preview);
            maybe_insert_bool(&mut input, "enableDocEmbedding", args.enable_doc_embedding);

            if input.len() == 1 {
                bail!("no update fields were provided");
            }

            let data = client
                .graphql(
                    queries::UPDATE_WORKSPACE_QUERY,
                    Some("updateWorkspace"),
                    json!({ "input": input }),
                )
                .await?;
            print_json_pretty(&data)
        }
        WorkspaceCommand::Delete { id } => {
            let data = client
                .graphql(
                    queries::DELETE_WORKSPACE_QUERY,
                    Some("deleteWorkspace"),
                    json!({ "id": id }),
                )
                .await?;
            print_json_pretty(&data)
        }
    }
}

async fn handle_doc(command: DocCommand, client: &AffineClient) -> Result<()> {
    match command {
        DocCommand::List(args) => {
            let data = client
                .graphql(
                    queries::LIST_DOCS_QUERY,
                    Some("listDocs"),
                    json!({
                        "workspaceId": args.workspace_id,
                        "pagination": {
                            "first": args.first,
                            "after": args.after,
                            "offset": args.offset,
                        }
                    }),
                )
                .await?;

            if args.resolve {
                let enriched = resolve_doc_list_titles(client, &args.workspace_id, &data).await?;
                print_json_pretty(&enriched)
            } else {
                print_json_pretty(&data)
            }
        }
        DocCommand::Recent(args) => {
            let data = client
                .graphql(
                    queries::LIST_RECENT_DOCS_QUERY,
                    Some("listRecentDocs"),
                    json!({
                        "workspaceId": args.workspace_id,
                        "pagination": {
                            "first": args.first,
                            "after": args.after,
                            "offset": args.offset,
                        }
                    }),
                )
                .await?;
            print_json_pretty(&data)
        }
        DocCommand::PublicList { workspace_id } => {
            let data = client
                .graphql(
                    queries::LIST_PUBLIC_DOCS_QUERY,
                    Some("listPublicDocs"),
                    json!({ "workspaceId": workspace_id }),
                )
                .await?;
            print_json_pretty(&data)
        }
        DocCommand::Get {
            workspace_id,
            doc_id,
        } => {
            let data = client
                .graphql(
                    queries::GET_DOC_QUERY,
                    Some("getDoc"),
                    json!({
                        "workspaceId": workspace_id,
                        "docId": doc_id,
                    }),
                )
                .await?;
            print_json_pretty(&data)
        }
        DocCommand::Search(args) => {
            let data = client
                .graphql(
                    queries::SEARCH_DOCS_QUERY,
                    Some("searchDocs"),
                    json!({
                        "id": args.workspace_id,
                        "input": {
                            "keyword": args.keyword,
                            "limit": args.limit,
                        }
                    }),
                )
                .await?;
            print_json_pretty(&data)
        }
        DocCommand::Analytics(args) => {
            let mut input = Map::new();
            if let Some(window_days) = args.window_days {
                input.insert("windowDays".to_owned(), Value::from(window_days));
            }
            if let Some(tz) = args.timezone {
                input.insert("timezone".to_owned(), Value::String(tz));
            }
            let vars = if input.is_empty() {
                json!({
                    "workspaceId": args.workspace_id,
                    "docId": args.doc_id,
                    "input": Value::Null,
                })
            } else {
                json!({
                    "workspaceId": args.workspace_id,
                    "docId": args.doc_id,
                    "input": input,
                })
            };
            let data = client
                .graphql(
                    queries::GET_DOC_ANALYTICS_QUERY,
                    Some("getDocAnalytics"),
                    vars,
                )
                .await?;
            print_json_pretty(&data)
        }
        DocCommand::Publish(args) => {
            let data = client
                .graphql(
                    queries::PUBLISH_DOC_QUERY,
                    Some("publishDoc"),
                    json!({
                        "workspaceId": args.workspace_id,
                        "docId": args.doc_id,
                        "mode": args.mode.as_ref().map(|m| m.as_graphql()),
                    }),
                )
                .await?;
            print_json_pretty(&data)
        }
        DocCommand::Unpublish {
            workspace_id,
            doc_id,
        } => {
            let data = client
                .graphql(
                    queries::REVOKE_PUBLIC_DOC_QUERY,
                    Some("revokePublicDoc"),
                    json!({
                        "workspaceId": workspace_id,
                        "docId": doc_id,
                    }),
                )
                .await?;
            print_json_pretty(&data)
        }
        DocCommand::Role(sub) => handle_doc_role(sub, client).await,
        DocCommand::Trash {
            workspace_id,
            doc_id,
        } => {
            let outcome = sync::delete_doc(client, &workspace_id, &doc_id).await?;
            print_json_pretty(&json!({
                "trashed": true,
                "workspaceId": workspace_id,
                "docId": doc_id,
                "server": outcome,
            }))
        }
    }
}

async fn handle_doc_role(command: DocRoleCommand, client: &AffineClient) -> Result<()> {
    match command {
        DocRoleCommand::Grant(args) => {
            let input = json!({
                "workspaceId": args.workspace_id,
                "docId": args.doc_id,
                "userIds": args.users,
                "role": args.role.as_graphql(),
            });
            let data = client
                .graphql(
                    queries::GRANT_DOC_USER_ROLES_QUERY,
                    Some("grantDocUserRoles"),
                    json!({ "input": input }),
                )
                .await?;
            print_json_pretty(&data)
        }
        DocRoleCommand::Update(args) => {
            let input = json!({
                "workspaceId": args.workspace_id,
                "docId": args.doc_id,
                "userId": args.user,
                "role": args.role.as_graphql(),
            });
            let data = client
                .graphql(
                    queries::UPDATE_DOC_USER_ROLE_QUERY,
                    Some("updateDocUserRole"),
                    json!({ "input": input }),
                )
                .await?;
            print_json_pretty(&data)
        }
        DocRoleCommand::Revoke(args) => {
            let input = json!({
                "workspaceId": args.workspace_id,
                "docId": args.doc_id,
                "userId": args.user,
            });
            let data = client
                .graphql(
                    queries::REVOKE_DOC_USER_ROLES_QUERY,
                    Some("revokeDocUserRoles"),
                    json!({ "input": input }),
                )
                .await?;
            print_json_pretty(&data)
        }
        DocRoleCommand::Default(args) => {
            let input = json!({
                "workspaceId": args.workspace_id,
                "docId": args.doc_id,
                "role": args.role.as_graphql(),
            });
            let data = client
                .graphql(
                    queries::UPDATE_DOC_DEFAULT_ROLE_QUERY,
                    Some("updateDocDefaultRole"),
                    json!({ "input": input }),
                )
                .await?;
            print_json_pretty(&data)
        }
    }
}

async fn resolve_doc_list_titles(
    client: &AffineClient,
    workspace_id: &str,
    list_response: &Value,
) -> Result<Value> {
    let mut enriched = list_response.clone();
    let edges = enriched
        .get_mut("workspace")
        .and_then(|w| w.get_mut("docs"))
        .and_then(|d| d.get_mut("edges"))
        .and_then(|e| e.as_array_mut());

    let Some(edges) = edges else {
        return Ok(enriched);
    };

    for edge in edges.iter_mut() {
        let Some(node) = edge.get_mut("node") else { continue };
        if node.get("title").and_then(Value::as_str).is_some() {
            continue;
        }
        let Some(doc_id) = node.get("id").and_then(Value::as_str).map(str::to_owned) else {
            continue;
        };

        let fetched = client
            .graphql(
                queries::GET_DOC_QUERY,
                Some("getDoc"),
                json!({ "workspaceId": workspace_id, "docId": doc_id }),
            )
            .await
            .ok();

        if let Some(fetched) = fetched {
            if let Some(doc) = fetched.get("workspace").and_then(|w| w.get("doc")) {
                for field in ["title", "summary"] {
                    if let Some(v) = doc.get(field) {
                        if !v.is_null() {
                            node[field] = v.clone();
                        }
                    }
                }
            }
        }
    }

    Ok(enriched)
}

async fn handle_blob(command: BlobCommand, client: &AffineClient) -> Result<()> {
    match command {
        BlobCommand::List { workspace_id } => {
            let data = client
                .graphql(
                    queries::LIST_BLOBS_QUERY,
                    Some("listBlobs"),
                    json!({ "workspaceId": workspace_id }),
                )
                .await?;
            print_json_pretty(&data)
        }
        BlobCommand::Upload(args) => handle_blob_upload(args, client).await,
        BlobCommand::Download {
            workspace_id,
            key,
            output,
        } => {
            let destination = output.unwrap_or_else(|| PathBuf::from(&key));
            let download = client.download_blob(&workspace_id, &key).await?;
            tokio::fs::write(&destination, &download.bytes)
                .await
                .with_context(|| format!("failed to write blob to {}", destination.display()))?;

            print_json_pretty(&json!({
                "key": key,
                "output": destination.display().to_string(),
                "bytes": download.bytes.len(),
                "contentType": download.content_type,
            }))
        }
        BlobCommand::Head { workspace_id, key } => {
            let head = client.head_blob(&workspace_id, &key).await?;
            print_json_pretty(&json!({
                "key": key,
                "status": head.status.as_u16(),
                "contentLength": head.content_length,
                "contentType": head.content_type,
                "etag": head.etag,
                "lastModified": head.last_modified,
            }))
        }
        BlobCommand::Usage { workspace_id } => {
            let data = client
                .graphql(
                    queries::BLOB_USAGE_QUERY,
                    Some("blobUsage"),
                    json!({ "workspaceId": workspace_id }),
                )
                .await?;
            print_json_pretty(&data)
        }
        BlobCommand::Release { workspace_id } => {
            let data = client
                .graphql(
                    queries::RELEASE_DELETED_BLOBS_QUERY,
                    Some("releaseDeletedBlobs"),
                    json!({ "workspaceId": workspace_id }),
                )
                .await?;
            print_json_pretty(&data)
        }
        BlobCommand::AbortUpload {
            workspace_id,
            key,
            upload_id,
        } => {
            let data = client
                .graphql(
                    queries::ABORT_BLOB_UPLOAD_QUERY,
                    Some("abortBlobUpload"),
                    json!({
                        "workspaceId": workspace_id,
                        "key": key,
                        "uploadId": upload_id,
                    }),
                )
                .await?;
            print_json_pretty(&data)
        }
        BlobCommand::Delete {
            workspace_id,
            key,
            permanently,
        } => {
            let data = client
                .graphql(
                    queries::DELETE_BLOB_QUERY,
                    Some("deleteBlob"),
                    json!({
                        "workspaceId": workspace_id,
                        "key": key,
                        "permanently": permanently,
                    }),
                )
                .await?;
            print_json_pretty(&data)
        }
    }
}

async fn handle_blob_upload(args: BlobUploadArgs, client: &AffineClient) -> Result<()> {
    let metadata = tokio::fs::metadata(&args.file)
        .await
        .with_context(|| format!("failed to stat upload file {}", args.file.display()))?;
    let size = metadata.len();
    let mime = args
        .mime
        .clone()
        .unwrap_or_else(|| {
            mime_guess::from_path(&args.file)
                .first_or_octet_stream()
                .to_string()
        });

    let file_name_default = args
        .file
        .file_name()
        .and_then(|n| n.to_str())
        .map(str::to_owned);
    let key = args
        .key
        .clone()
        .or(file_name_default.clone())
        .ok_or_else(|| anyhow!("unable to derive a blob key; pass --key"))?;

    let mode = resolve_upload_mode(&args.mode, size);

    if matches!(mode, BlobUploadMode::Graphql) {
        let data = client
            .graphql_upload(
                queries::SET_BLOB_QUERY,
                "setBlob",
                json!({ "workspaceId": args.workspace_id }),
                "blob",
                &args.file,
            )
            .await?;
        return print_json_pretty(&data);
    }

    // Everything below asks the server for an upload plan. For PRESIGNED it hands us
    // a single URL; for MULTIPART it gives us a part size and we request presigned
    // URLs per part.
    let init = client
        .graphql(
            queries::CREATE_BLOB_UPLOAD_QUERY,
            Some("createBlobUpload"),
            json!({
                "workspaceId": args.workspace_id,
                "key": key,
                "mime": mime,
                "size": size as i64,
            }),
        )
        .await?;
    let init = init
        .get("createBlobUpload")
        .cloned()
        .ok_or_else(|| anyhow!("createBlobUpload returned no payload"))?;

    if init.get("alreadyUploaded").and_then(Value::as_bool).unwrap_or(false) {
        return print_json_pretty(&json!({
            "key": init.get("blobKey").cloned().unwrap_or(Value::String(key)),
            "alreadyUploaded": true,
        }));
    }

    let method = init
        .get("method")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("createBlobUpload response missing `method`"))?;

    // The server can downgrade a client-requested multipart/presigned upload back to
    // GRAPHQL when the backend is not configured for object storage. Fall back for
    // `auto`, but surface an error if the caller explicitly asked for a different path.
    if method == "GRAPHQL" {
        if matches!(mode, BlobUploadMode::Presigned | BlobUploadMode::Multipart) {
            bail!(
                "server responded with upload method GRAPHQL; rerun with `--mode graphql` or `auto`"
            );
        }
        let data = client
            .graphql_upload(
                queries::SET_BLOB_QUERY,
                "setBlob",
                json!({ "workspaceId": args.workspace_id }),
                "blob",
                &args.file,
            )
            .await?;
        return print_json_pretty(&data);
    }

    let bytes = tokio::fs::read(&args.file)
        .await
        .with_context(|| format!("failed to read upload file {}", args.file.display()))?;
    let blob_key = init
        .get("blobKey")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| key.clone());

    let completion = match (method, &mode) {
        ("MULTIPART", _) | (_, BlobUploadMode::Multipart) => {
            upload_multipart(client, &args.workspace_id, &blob_key, &init, bytes).await?
        }
        _ => upload_presigned(client, &args.workspace_id, &blob_key, &init, bytes).await?,
    };
    print_json_pretty(&completion)
}

fn resolve_upload_mode(requested: &BlobUploadMode, size: u64) -> BlobUploadMode {
    match requested {
        BlobUploadMode::Auto => {
            if size < LARGE_BLOB_BYTES {
                BlobUploadMode::Graphql
            } else {
                BlobUploadMode::Presigned
            }
        }
        other => other.clone(),
    }
}

async fn upload_presigned(
    client: &AffineClient,
    workspace_id: &str,
    key: &str,
    init: &Value,
    bytes: Vec<u8>,
) -> Result<Value> {
    let url = init
        .get("uploadUrl")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("createBlobUpload/PRESIGNED response missing uploadUrl"))?;
    let headers = init.get("headers").and_then(Value::as_object);
    let outcome = client.put_presigned(url, bytes, headers).await?;

    let complete = client
        .graphql(
            queries::COMPLETE_BLOB_UPLOAD_QUERY,
            Some("completeBlobUpload"),
            json!({
                "workspaceId": workspace_id,
                "key": key,
                "uploadId": init.get("uploadId").cloned().unwrap_or(Value::Null),
                "parts": Value::Null,
            }),
        )
        .await?;

    Ok(json!({
        "mode": "presigned",
        "key": key,
        "presignedStatus": outcome.status.as_u16(),
        "etag": outcome.etag,
        "complete": complete,
    }))
}

async fn upload_multipart(
    client: &AffineClient,
    workspace_id: &str,
    key: &str,
    init: &Value,
    bytes: Vec<u8>,
) -> Result<Value> {
    let upload_id = init
        .get("uploadId")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("createBlobUpload/MULTIPART missing uploadId"))?
        .to_owned();

    // Default to a reasonable part size when the server doesn't pin one.
    let part_size = init
        .get("partSize")
        .and_then(Value::as_u64)
        .unwrap_or(8 * 1024 * 1024) as usize;
    if part_size == 0 {
        bail!("server returned partSize=0");
    }

    // Skip parts that an earlier run already uploaded.
    let mut already_uploaded = std::collections::HashMap::new();
    if let Some(parts) = init.get("uploadedParts").and_then(Value::as_array) {
        for part in parts {
            if let (Some(n), Some(e)) = (
                part.get("partNumber").and_then(Value::as_i64),
                part.get("etag").and_then(Value::as_str),
            ) {
                already_uploaded.insert(n as i64, e.to_owned());
            }
        }
    }

    let mut completed_parts = Vec::new();
    let total_parts = bytes.len().div_ceil(part_size);
    for part_number_zero_based in 0..total_parts {
        let part_number = (part_number_zero_based + 1) as i64;
        let start = part_number_zero_based * part_size;
        let end = usize::min(start + part_size, bytes.len());
        let chunk = bytes[start..end].to_vec();

        if let Some(etag) = already_uploaded.get(&part_number) {
            completed_parts.push(json!({ "partNumber": part_number, "etag": etag }));
            continue;
        }

        let part_url = client
            .graphql(
                queries::BLOB_UPLOAD_PART_URL_QUERY,
                Some("blobUploadPartUrl"),
                json!({
                    "workspaceId": workspace_id,
                    "key": key,
                    "uploadId": upload_id,
                    "partNumber": part_number,
                }),
            )
            .await?;
        let part = part_url
            .get("workspace")
            .and_then(|w| w.get("blobUploadPartUrl"))
            .ok_or_else(|| anyhow!("blobUploadPartUrl returned no payload"))?;
        let url = part
            .get("uploadUrl")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("blobUploadPartUrl missing uploadUrl"))?;
        let headers = part.get("headers").and_then(Value::as_object);

        let outcome = client.put_presigned(url, chunk, headers).await?;
        let etag = outcome
            .etag
            .ok_or_else(|| anyhow!("presigned PUT returned no ETag; cannot complete multipart"))?;
        completed_parts.push(json!({ "partNumber": part_number, "etag": etag }));
    }

    let complete = client
        .graphql(
            queries::COMPLETE_BLOB_UPLOAD_QUERY,
            Some("completeBlobUpload"),
            json!({
                "workspaceId": workspace_id,
                "key": key,
                "uploadId": upload_id,
                "parts": completed_parts,
            }),
        )
        .await?;

    Ok(json!({
        "mode": "multipart",
        "key": key,
        "uploadId": upload_id,
        "parts": total_parts,
        "complete": complete,
    }))
}

fn build_client(cli: &CliContext, config: &ConfigFile) -> Result<AffineClient> {
    let server = resolve_server(cli.server.as_deref(), config.server.as_deref())?;
    let auth = resolved_auth(cli, config);
    if matches!(auth, AuthState::None) {
        bail!("no authentication configured; pass --token or run `affine-cli auth login` first");
    }

    AffineClient::new(server, cli.client_version.clone(), auth)
}

fn resolved_auth(cli: &CliContext, config: &ConfigFile) -> AuthState {
    if let Some(token) = &cli.token {
        AuthState::Bearer(token.clone())
    } else if !config.cookies.is_empty() {
        AuthState::Cookies(config.cookies.clone())
    } else {
        AuthState::None
    }
}

fn resolve_server(cli_server: Option<&str>, config_server: Option<&str>) -> Result<String> {
    cli_server
        .or(config_server)
        .map(str::to_owned)
        .ok_or_else(|| {
            anyhow!("no AFFiNE server configured; pass --server or run `affine-cli auth login`")
        })
}

fn parse_optional_json_object(raw: Option<&str>) -> Result<Map<String, Value>> {
    let Some(raw) = raw else {
        return Ok(Map::new());
    };

    let value: Value = serde_json::from_str(raw).context("failed to parse --variables JSON")?;
    value
        .as_object()
        .cloned()
        .ok_or_else(|| anyhow!("--variables must be a JSON object"))
}

fn maybe_insert_bool(target: &mut Map<String, Value>, key: &str, value: Option<bool>) {
    if let Some(value) = value {
        target.insert(key.to_owned(), Value::Bool(value));
    }
}

fn print_json_pretty(value: &Value) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn urlencoding_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

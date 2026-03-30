mod client;
mod config;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, Parser, Subcommand};
use serde_json::{Map, Value, json};

use client::{AffineClient, AuthState};
use config::{ConfigFile, default_config_path};

const DEFAULT_CLIENT_VERSION: &str = "0.25.0";

const CURRENT_USER_QUERY: &str = r#"
query getCurrentUser {
  currentUser {
    id
    name
    email
    emailVerified
    avatarUrl
    hasPassword
  }
}
"#;

const LIST_WORKSPACES_QUERY: &str = r#"
query getWorkspaces {
  workspaces {
    id
    initialized
    team
    public
    role
    createdAt
    enableAi
    enableSharing
    enableUrlPreview
    enableDocEmbedding
    memberCount
    owner {
      id
      name
      email
    }
  }
}
"#;

const GET_WORKSPACE_QUERY: &str = r#"
query getWorkspace($id: String!) {
  workspace(id: $id) {
    id
    initialized
    team
    public
    role
    createdAt
    enableAi
    enableSharing
    enableUrlPreview
    enableDocEmbedding
    memberCount
    inviteLink {
      link
      expireTime
    }
    owner {
      id
      name
      email
      avatarUrl
    }
    quota {
      name
      blobLimit
      storageQuota
      usedStorageQuota
      historyPeriod
      memberLimit
      memberCount
      overcapacityMemberCount
      humanReadable {
        name
        blobLimit
        storageQuota
        historyPeriod
        memberLimit
        memberCount
      }
    }
  }
}
"#;

const CREATE_WORKSPACE_QUERY: &str = r#"
mutation createWorkspace {
  createWorkspace {
    id
    public
    createdAt
    initialized
  }
}
"#;

const CREATE_WORKSPACE_WITH_INIT_QUERY: &str = r#"
mutation createWorkspace($init: Upload!) {
  createWorkspace(init: $init) {
    id
    public
    createdAt
    initialized
  }
}
"#;

const UPDATE_WORKSPACE_QUERY: &str = r#"
mutation updateWorkspace($input: UpdateWorkspaceInput!) {
  updateWorkspace(input: $input) {
    id
    public
    enableAi
    enableSharing
    enableUrlPreview
    enableDocEmbedding
  }
}
"#;

const DELETE_WORKSPACE_QUERY: &str = r#"
mutation deleteWorkspace($id: String!) {
  deleteWorkspace(id: $id)
}
"#;

const LIST_DOCS_QUERY: &str = r#"
query listDocs($workspaceId: String!, $pagination: PaginationInput!) {
  workspace(id: $workspaceId) {
    docs(pagination: $pagination) {
      totalCount
      pageInfo {
        startCursor
        endCursor
        hasNextPage
        hasPreviousPage
      }
      edges {
        cursor
        node {
          id
          title
          public
          mode
          defaultRole
          summary
          createdAt
          updatedAt
          creatorId
          lastUpdaterId
          workspaceId
        }
      }
    }
  }
}
"#;

const GET_DOC_QUERY: &str = r#"
query getDoc($workspaceId: String!, $docId: String!) {
  workspace(id: $workspaceId) {
    doc(docId: $docId) {
      id
      title
      public
      mode
      defaultRole
      summary
      createdAt
      updatedAt
      creatorId
      lastUpdaterId
      workspaceId
      createdBy {
        id
        name
        avatarUrl
      }
      lastUpdatedBy {
        id
        name
        avatarUrl
      }
      meta {
        createdAt
        updatedAt
        createdBy {
          name
          avatarUrl
        }
        updatedBy {
          name
          avatarUrl
        }
      }
    }
  }
}
"#;

const SEARCH_DOCS_QUERY: &str = r#"
query searchDocs($id: String!, $input: SearchDocsInput!) {
  workspace(id: $id) {
    searchDocs(input: $input) {
      docId
      title
      blockId
      highlight
      createdAt
      updatedAt
      createdByUser {
        id
        name
        avatarUrl
      }
      updatedByUser {
        id
        name
        avatarUrl
      }
    }
  }
}
"#;

const LIST_BLOBS_QUERY: &str = r#"
query listBlobs($workspaceId: String!) {
  workspace(id: $workspaceId) {
    blobs {
      key
      size
      mime
      createdAt
    }
  }
}
"#;

const SET_BLOB_QUERY: &str = r#"
mutation setBlob($workspaceId: String!, $blob: Upload!) {
  setBlob(workspaceId: $workspaceId, blob: $blob)
}
"#;

const DELETE_BLOB_QUERY: &str = r#"
mutation deleteBlob($workspaceId: String!, $key: String!, $permanently: Boolean) {
  deleteBlob(workspaceId: $workspaceId, key: $key, permanently: $permanently)
}
"#;

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
}

#[derive(Args, Debug)]
struct LoginArgs {
    email: String,
    #[arg(long)]
    password: Option<String>,
    #[arg(long)]
    callback_url: Option<String>,
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
    Get {
        workspace_id: String,
        doc_id: String,
    },
    Search(DocSearchArgs),
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
}

#[derive(Args, Debug)]
struct DocSearchArgs {
    workspace_id: String,
    keyword: String,
    #[arg(long, default_value_t = 20)]
    limit: i64,
}

#[derive(Subcommand, Debug)]
enum BlobCommand {
    List {
        workspace_id: String,
    },
    Upload {
        workspace_id: String,
        file: PathBuf,
    },
    Download {
        workspace_id: String,
        key: String,
        #[arg(long)]
        output: Option<PathBuf>,
    },
    Delete {
        workspace_id: String,
        key: String,
        #[arg(long)]
        permanently: bool,
    },
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
            let response = client
                .sign_in(
                    &args.email,
                    args.password.as_deref(),
                    args.callback_url.as_deref(),
                )
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
                .graphql(CURRENT_USER_QUERY, Some("getCurrentUser"), json!({}))
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
                .graphql(LIST_WORKSPACES_QUERY, Some("getWorkspaces"), json!({}))
                .await?;
            print_json_pretty(&data)
        }
        WorkspaceCommand::Get { id } => {
            let data = client
                .graphql(
                    GET_WORKSPACE_QUERY,
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
                        CREATE_WORKSPACE_WITH_INIT_QUERY,
                        "createWorkspace",
                        json!({}),
                        "init",
                        &init,
                    )
                    .await?
            } else {
                client
                    .graphql(CREATE_WORKSPACE_QUERY, Some("createWorkspace"), json!({}))
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
                    UPDATE_WORKSPACE_QUERY,
                    Some("updateWorkspace"),
                    json!({ "input": input }),
                )
                .await?;
            print_json_pretty(&data)
        }
        WorkspaceCommand::Delete { id } => {
            let data = client
                .graphql(
                    DELETE_WORKSPACE_QUERY,
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
                    LIST_DOCS_QUERY,
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
            print_json_pretty(&data)
        }
        DocCommand::Get {
            workspace_id,
            doc_id,
        } => {
            let data = client
                .graphql(
                    GET_DOC_QUERY,
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
                    SEARCH_DOCS_QUERY,
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
    }
}

async fn handle_blob(command: BlobCommand, client: &AffineClient) -> Result<()> {
    match command {
        BlobCommand::List { workspace_id } => {
            let data = client
                .graphql(
                    LIST_BLOBS_QUERY,
                    Some("listBlobs"),
                    json!({ "workspaceId": workspace_id }),
                )
                .await?;
            print_json_pretty(&data)
        }
        BlobCommand::Upload { workspace_id, file } => {
            let data = client
                .graphql_upload(
                    SET_BLOB_QUERY,
                    "setBlob",
                    json!({ "workspaceId": workspace_id }),
                    "blob",
                    &file,
                )
                .await?;
            print_json_pretty(&data)
        }
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
        BlobCommand::Delete {
            workspace_id,
            key,
            permanently,
        } => {
            let data = client
                .graphql(
                    DELETE_BLOB_QUERY,
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

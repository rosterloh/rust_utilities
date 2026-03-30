# AFFiNE Cli

CLI for the AFFiNE GraphQL API and workspace blob endpoints

## Usage

```bash
  cargo run -p affine-cli -- --server https://your-affine-instance.com auth login you@example.com --password 'secret'
  cargo run -p affine-cli -- workspace list
  cargo run -p affine-cli -- doc list <workspace-id> --first 10
  cargo run -p affine-cli -- blob upload <workspace-id> ./image.png
```

You can also authenticate with an access token instead of a saved session:
```bash
  AFFINE_BASE_URL=https://your-affine-instance.com \
  AFFINE_API_TOKEN=your_access_token \
  cargo run -p affine-cli -- auth whoami
```

Follow [these](https://www.mintlify.com/toeverything/AFFiNE/api/authentication) instructions to get an access token

## Links

- [API Docs](https://www.mintlify.com/toeverything/AFFiNE/api/overview)

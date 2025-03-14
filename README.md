# glab-to-gh

> Migrate repos from GitLab to GitHub

Individual repos or all repos from a GitLab user or group can be selected. The repos are
sequentially cloned to a local folder and pushed to GitHub.

Things copied:

- Full Git history (`git clone --mirror` + `git push --mirror`)
- Name (`subgroup/project` becomes `subgroup_project`)
- Description
- Visibility (if `--visibility` is `inherit`)
- Archived status
- Default branch

Also see programs in src/bin for archiving projects, getting GitLab project storage sizes, and
updating local git origin remote URLs

## Issues

Has some issues with LFS currently

## Usage

```text
Usage: glab-to-gh [OPTIONS] <--repo <REPO>|--su|--sg <GROUP>> <--tu|--to <ORG>>

Options:
      --glab-host <HOST>    Source GitLab hostname [env: GLAB_HOST=] [default: gitlab.com]
  -r, --repo <REPO>         Source GitLab project path or full URL; can be a list or specified multiple times
      --su                  Source GitLab user; migrate all repos [aliases: source-user]
      --sg <GROUP>          Source GitLab group or group/subgroup; migrate all repos [aliases: source-group]
      --gh-host <HOST>      Target GitHub hostname [env: GH_HOST=] [default: api.github.com]
      --tu                  Target GitHub user [aliases: target-user]
      --to <ORG>            Target GitHub org [aliases: target-org]
  -v, --visibility <VIS>    Migrated repo visibility [default: inherit] [possible values: inherit, public, internal, private]
      --glab-token <TOKEN>  GitLab access token [env: GLAB_TOKEN]
      --gh-token <TOKEN>    GitHub access token [env: GH_TOKEN]
  -h, --help                Print help
```

Provide environment variables `GLAB_TOKEN` and `GH_TOKEN`, or enter those values into a `.env` file
in the same folder as the binary. The tokens are used for both API access and git `clone`/`push`.

- The GitLab access token requires at least `read_api` and `read_repository` scope.
- The GitHub access token requires at least repository `Administration` and `Contents` read/write
  permissions. Ensure the token is scoped to the correct user or org.

## Example

Copy all repos from a GitLab group to a GitHub org, and make all repos private:

```sh
cargo run -- -sg example-group --to example-org -v private
#             └source group      └target org     └visibility
```

## Update git origin URLs after migration

See the [update-origin-url](./src/bin/update-origin-url.rs) bin. Expects migrated repo names to
match their GitLab source.

## Dev

- Lint, run unit tests, and run a development build:

  ```sh
  cargo clippy
  cargo test
  cargo run -- --help
  ```

- Run a release build:

  ```sh
  cargo run --release -- --help
  ```

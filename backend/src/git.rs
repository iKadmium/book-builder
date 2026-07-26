use std::path::Path;

use git2::{Cred, FetchOptions, RemoteCallbacks, Repository, ResetType, build::RepoBuilder};

/// Clone the repo if `data_dir` doesn't exist, otherwise fetch and hard-reset
/// to the remote HEAD so the local copy is always up to date.
pub fn sync_repo(url: &str, oauth_token: &str, data_dir: &Path) -> Result<(), git2::Error> {
    if data_dir.join(".git").exists() {
        tracing::info!("Repo exists at {}, pulling latest", data_dir.display());
        let repo = Repository::open(data_dir)?;
        pull(&repo, oauth_token)
    } else {
        tracing::info!("Cloning {url} into {}", data_dir.display());
        clone(url, oauth_token, data_dir)?;
        Ok(())
    }
}

fn make_fetch_opts(oauth_token: &str) -> FetchOptions<'_> {
    // git2 requires the callback closure to own its captures, so we copy the
    // token string for the duration of the fetch.
    let token = oauth_token.to_string();
    let mut callbacks = RemoteCallbacks::new();
    callbacks.credentials(move |_url, _username, _allowed| {
        // Forgejo accepts any non-empty username with the OAuth2 token as password.
        Cred::userpass_plaintext("git", &token)
    });

    let mut opts = FetchOptions::new();
    opts.remote_callbacks(callbacks);
    opts
}

fn clone(url: &str, oauth_token: &str, path: &Path) -> Result<Repository, git2::Error> {
    RepoBuilder::new()
        .fetch_options(make_fetch_opts(oauth_token))
        .clone(url, path)
}

/// Pull the latest changes for an already-cloned repo at `data_dir`.
pub fn pull_repo(data_dir: &Path, oauth_token: &str) -> Result<(), git2::Error> {
    let repo = Repository::open(data_dir)?;
    pull(&repo, oauth_token)
}

fn pull(repo: &Repository, oauth_token: &str) -> Result<(), git2::Error> {
    let mut remote = repo.find_remote("origin")?;
    remote.fetch(
        &[] as &[&str],
        Some(&mut make_fetch_opts(oauth_token)),
        None,
    )?;

    // Hard-reset the working tree to whatever the remote just sent.
    let fetch_head = repo.find_reference("FETCH_HEAD")?;
    let commit = fetch_head.peel_to_commit()?;
    repo.reset(commit.as_object(), ResetType::Hard, None)?;

    tracing::info!("Repo updated to {}", commit.id());
    Ok(())
}

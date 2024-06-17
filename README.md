# rust-config

Common CI and crate configuration for my Rust projects. This is meant to be pulled in as a separate git remote in your repo.

Initial setup for a repo:

```
git remote add ci git@github.com:LucasPickering/rust-config.git
git fetch ci
git cherry-pick ci/master
```

Pulling in new chains:

```
git fetch ci
git merge ci/master
```

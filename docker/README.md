# Agent workspace

A container with the localfiles monorepo mounted, the toolchains to build
the portable halves of the apps, and no credentials in the image.

```
cd docker
./bootstrap.sh                      # once: writes .env
docker compose build
docker compose up -d
docker compose exec agent bash      # run again for a second agent
```

Then once, inside the container — all of it persists in Docker volumes:

```
ssh-keygen -t ed25519 -C '<you>'
gh auth login                       # choose SSH as the git protocol
gh ssh-key add ~/.ssh/id_ed25519.pub
claude                              # /login on first run
```

`up -d` plus `exec`, not `run --rm`: one long-lived container means one
`ssh-add`, shared by every shell and every coding agent you exec into it.
`docker compose run --rm agent bash` still works for a throwaway shell.

## Host layout it assumes

```
~/localfiles/          <- the monorepo, mounted at /work
  docker/              <- here
  apps/{gallery,contacts,music,health}/
  core/  shells/       <- shared Rust cores and GTK shells
  conformance/  docs/  .agents/
```

`bootstrap.sh` derives `WORKSPACE` from its own location (the directory
above `docker/`). If the checkout lives elsewhere, edit `WORKSPACE` in
`.env`. There is no `agent-home/` any more — see below.

## What is mounted, and why that way

| | Container path | Why |
|---|---|---|
| bind `$WORKSPACE` | `/work` | The monorepo. Spec, docker, and all four apps. |
| volume `ssh` | `/home/agent/.ssh` | keys, `known_hosts` |
| volume `config` | `/home/agent/.config` | `gh` login, git global config, agent CLI config |
| volume `claude` | `/home/agent/.claude` | Claude Code state — it does not use XDG |
| volume `cursor` | `/home/agent/.cursor` | Cursor chats, model prefs, CLI state — also not XDG |
| volume `cache` | `/home/agent/.cache` | cargo, rustup, Go, npm, the ORT static lib. Gigabytes. |

**`/home/agent/.local` is deliberately not mounted.** It holds `pi` and
`cursor-agent`, installed by the image. A volume there would pin both at
whatever version existed when the volume was first created, and rebuilding
would silently not upgrade them. This is not hypothetical: the previous
version of this file bind-mounted the whole of `/home/agent`, which shadowed
both binaries completely — the build asserted they were on PATH and the
running container did not have them.

Named volumes rather than host directories because the container writes these
as uid 1000 and nothing on the host needs to read them. A named volume
mounted over a path that exists in the image is initialised from that path,
*ownership included*, which is why the Dockerfile creates them as `agent`
and why no root entrypoint is needed to chown anything.

`docker compose down` keeps the volumes. **`down -v` destroys them** — that is
the command that deletes your ssh key and logs you out of everything.

## Where the tools come from

Everything Fedora packages comes from `dnf`. Two exceptions, in descending
order of comfort: `pi` from npm at a pinned version, and `cursor-agent` from
`cursor.com/install`, the only unpinned step in the image (`INSTALL_CURSOR=0`
drops it).

Required packages fail the build if a name is wrong. `helix`, `eza`, `btop`,
and `EXTRA_PACKAGES` use `--skip-unavailable` so they do not.

`EXTRA_PACKAGES` adds extras without editing the Dockerfile:

```
docker compose build --build-arg EXTRA_PACKAGES="tig bat fzf"
```

### zellij and lazygit

Neither is in the Fedora repositories. Rather than guess at a COPR name this
file cannot verify, install them at run time — both land in the `cache` volume
and survive restarts:

```
cargo install zellij
go install github.com/jesseduffield/lazygit@latest
```

`tmux` is installed as the packaged multiplexer so you are not without one
meanwhile.

## What it can and cannot build

Builds and tests here: both Rust workspaces in `core/` and `shells/`, every
Rust crate in `apps/gallery/core`, both gallery Linux configurations in
`apps/gallery/linux`, and all of `apps/health` (cgo + sqlite). The
conformance and documentation consistency checks also run here.

Cannot, ever: `xcodegen`, `xcodebuild`, the iOS slices of
`GalleryCore.xcframework`, and therefore every Swift test in gallery,
contacts and music. Those need Xcode on a Mac — [`mac/`](../mac/README.md).
An agent that changes Swift here has written unverified code and must say so.

Work-item routing: [`.agents/ROUTING.md`](../.agents/ROUTING.md).

An uncached Cargo bootstrap fetches crates.io sources. The current reviewed
ADR 0002 R13 build-time exception is `ort`, whose build script can also fetch
the ~85 MB prebuilt ONNX Runtime. It lands in the `cache` volume and is paid
once for all agents, not once per container; this does not permit runtime
networking. For an offline build, pre-populate Cargo's cache and set
`ORT_LIB_LOCATION` to a directory containing `libonnxruntime.a`.

## SECURITY

Keys now live in a volume inside the container rather than being reached
through a forwarded agent socket. That is a deliberate trade and it moves in
both directions, so be clear about which way.

**What got worse.** A private key is now readable by anything running in the
container, including agent-written code. The socket-forwarding model kept the
key on the host and exposed only the ability to *use* it while the container
ran; this model exposes the key itself.

**What got better.** Nothing on the host is reachable from the container any
more, and the blast radius is exactly one volume whose entire contents you
chose.

**The mitigation that makes this fine.** Generate the key *inside* the
container and register it on GitHub as its own key. It is then a container
credential, not your personal one: revoking it is one click, costs you nothing
else, and you never have to wonder what else it opened. Do not copy an
existing personal key into the volume — that throws away the only thing this
posture has going for it.

Same reasoning for `gh auth login`: authorise it for this monorepo
and nothing more. If you would rather use a token, a fine-grained PAT with
contents + pull-requests write on this repo can be exported as
`GH_TOKEN` before `compose up` — but note that `gh` ignores its stored login
whenever `GH_TOKEN` is set, so it is one or the other.

`known_hosts` is trust-on-first-use (`StrictHostKeyChecking accept-new`): the
first connection to github.com is accepted unseen and pinned, and a *changed*
key afterwards is refused loudly. To close the first-use gap, run
`ssh-keyscan github.com >> ~/.ssh/known_hosts` once and compare against
GitHub's published fingerprints.

`.env` holds no secrets under this design. It is gitignored and `chmod 600`
anyway.

## If something is wrong

The entrypoint checks the volumes on every start and prints a precise fix
rather than failing obscurely later. The two you may actually hit:

- **`~/.ssh is not writable`** — the volume was created with the wrong
  ownership, usually because it predates this compose file. The message names
  the exact `docker volume rm` or `chown` to run.
- **`dnf: unknown option --skip-unavailable`** — your base image is on dnf4
  rather than dnf5. That flag is only used on the optional extras line.

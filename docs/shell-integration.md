# Man pages and shell completions

Cargo generates the man pages and completion scripts in `assets/` whenever it
builds this source checkout. `cargo install --path .` installs the binary, not
these files. Install the assets before removing the checkout.

## Man pages

Install the generated section 1 man pages in your user data directory:

```sh
mkdir -p "$HOME/.local/share/man/man1"
cp assets/man/*.1 "$HOME/.local/share/man/man1/"
```

Open the main page or a subcommand page:

```sh
man xngmcp
man xngmcp-search
```

If `man xngmcp` cannot find the page, add the user directory to `MANPATH` in
your shell startup file, then start a new shell:

```sh
export MANPATH="$HOME/.local/share/man${MANPATH:+:$MANPATH}:"
```

The trailing colon preserves the system man-page path.

## Shell completions

The generated scripts cover Bash, Zsh, Fish, Elvish, and PowerShell. The
commands below install the common shells. Restart the shell after installation,
or source the file when the shell supports it.

### Bash

With `bash-completion` installed, place the script in its per-user completion
directory:

```sh
mkdir -p "$HOME/.local/share/bash-completion/completions"
cp assets/completions/xngmcp.bash \
  "$HOME/.local/share/bash-completion/completions/xngmcp"
```

To use it in the current shell, or if your Bash startup files do not load that
directory automatically, add this to `~/.bashrc`:

```sh
source "$HOME/.local/share/bash-completion/completions/xngmcp"
```

### Zsh

For plain Zsh, copy the `_xngmcp` function to a directory in `fpath`. Add the
`fpath` line before the existing `compinit` call in `~/.zshrc`.

```sh
mkdir -p "$HOME/.local/share/zsh/site-functions"
cp assets/completions/_xngmcp "$HOME/.local/share/zsh/site-functions/"
```

```zsh
fpath=("$HOME/.local/share/zsh/site-functions" $fpath)
autoload -Uz compinit
compinit
```

If your configuration already initializes completions, keep its `compinit` call
and add only the `fpath` line before it.

### Oh My Zsh

Oh My Zsh adds `$ZSH_CUSTOM/completions` to `fpath` before it initializes
`compinit`, so no `.zshrc` change is needed. Copy the generated Zsh function
there and start a new Zsh session:

```sh
omz_custom="${ZSH_CUSTOM:-${ZSH:-$HOME/.oh-my-zsh}/custom}"
mkdir -p "$omz_custom/completions"
cp assets/completions/_xngmcp "$omz_custom/completions/"
```

### Fish

Fish loads `~/.config/fish/completions` automatically:

```sh
mkdir -p "$HOME/.config/fish/completions"
cp assets/completions/xngmcp.fish "$HOME/.config/fish/completions/"
```

### Elvish

Copy the script and load it from `rc.elv` with `eval`:

```sh
mkdir -p "$HOME/.local/share/elvish"
cp assets/completions/xngmcp.elv "$HOME/.local/share/elvish/"
```

```elvish
eval (slurp < $E:HOME/.local/share/elvish/xngmcp.elv)
```

### PowerShell

Copy the generated script, create the current user's profile if needed, and
dot-source the script from that profile:

```powershell
$completion = Join-Path $HOME ".local/share/xngmcp/_xngmcp.ps1"
New-Item -ItemType Directory -Force (Split-Path -Parent $completion)
Copy-Item assets/completions/_xngmcp.ps1 $completion
if (!(Test-Path $PROFILE)) { New-Item -ItemType File -Path $PROFILE -Force }
```

Add this line to `$PROFILE`, then open a new PowerShell session:

```powershell
. "$HOME/.local/share/xngmcp/_xngmcp.ps1"
```

On Windows, PowerShell may require a `RemoteSigned` or less restrictive
execution policy before it loads profile scripts.

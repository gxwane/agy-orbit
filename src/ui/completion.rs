use clap_complete::Shell;
use colored::Colorize;
use std::io::Write;

/// Intelligently detect the current shell environment, strictly prioritizing
/// explicit $SHELL to avoid the Windows global PSModulePath trap.
pub fn detect_current_shell() -> Shell {
    // 1. Priority 1: Check $SHELL (Git Bash, WSL, MSYS2, macOS, Linux)
    if let Ok(shell_path) = std::env::var("SHELL") {
        let lower = shell_path.to_lowercase();
        if lower.ends_with("zsh") || lower.ends_with("zsh.exe") {
            return Shell::Zsh;
        }
        if lower.ends_with("bash") || lower.ends_with("bash.exe") {
            return Shell::Bash;
        }
        if lower.ends_with("fish") || lower.ends_with("fish.exe") {
            return Shell::Fish;
        }
        if lower.ends_with("elvish") || lower.ends_with("elvish.exe") {
            return Shell::Elvish;
        }
        if lower.ends_with("pwsh") || lower.ends_with("powershell") {
            return Shell::PowerShell;
        }
    }

    // 2. Priority 2: Windows Terminal / PowerShell session markers
    #[cfg(windows)]
    {
        if std::env::var("WT_SESSION").is_ok() || std::env::var("PSModulePath").is_ok() {
            return Shell::PowerShell;
        }
    }

    // 3. Priority 3: Operating system standard fallback
    if cfg!(windows) {
        Shell::PowerShell
    } else if cfg!(target_os = "macos") {
        Shell::Zsh
    } else {
        Shell::Bash
    }
}

/// Render a friendly, colored shell completion setup guide in an interactive terminal
pub fn render_completion_guide(detected: Shell) {
    let shell_name = match detected {
        Shell::PowerShell => "PowerShell",
        Shell::Bash => "Bash",
        Shell::Zsh => "Zsh",
        Shell::Fish => "Fish",
        Shell::Elvish => "Elvish",
        _ => "PowerShell",
    };

    println!(
        "{} Detected current shell: {}",
        "💡".yellow(),
        shell_name.bold().cyan()
    );
    println!();
    println!("{}", "Quick activation (current session only):".bold());
    match detected {
        Shell::PowerShell => {
            println!(
                "  {}",
                "agyo completion powershell | Out-String | Invoke-Expression".green()
            );
        }
        Shell::Bash => {
            println!("  {}", r#"source <(agyo completion bash)"#.green());
        }
        Shell::Zsh => {
            println!("  {}", r#"source <(agyo completion zsh)"#.green());
        }
        Shell::Fish => {
            println!("  {}", "agyo completion fish | source".green());
        }
        Shell::Elvish => {
            println!("  {}", "eval (agyo completion elvish | slurp)".green());
        }
        _ => {
            println!("  {}", "agyo completion <shell> | source".green());
        }
    }

    println!();
    println!(
        "{}",
        "Permanent activation (recommended, zero-latency cached):".bold()
    );
    match detected {
        Shell::PowerShell => {
            println!(
                "  1. {}",
                r#"agyo completion powershell > "$HOME\.agyo\completion.ps1""#.cyan()
            );
            println!(
                "  2. Add {} to your {}",
                r#". "$HOME\.agyo\completion.ps1""#.cyan(),
                "$PROFILE".yellow()
            );
        }
        Shell::Bash => {
            println!(
                "  {}",
                "agyo completion bash > /etc/bash_completion.d/agyo".cyan()
            );
            println!(
                "  (or add {} to {})",
                r#"source <(agyo completion bash)"#.cyan(),
                "~/.bashrc".yellow()
            );
        }
        Shell::Zsh => {
            println!(
                "  {}",
                r#"agyo completion zsh > "${fpath[1]}/_agyo""#.cyan()
            );
            println!(
                "  (or add {} to {})",
                r#"source <(agyo completion zsh)"#.cyan(),
                "~/.zshrc".yellow()
            );
        }
        Shell::Fish => {
            println!(
                "  {}",
                "agyo completion fish > ~/.config/fish/completions/agyo.fish".cyan()
            );
        }
        Shell::Elvish => {
            println!(
                "  {}",
                "agyo completion elvish > ~/.elvish/lib/agyo.elv".cyan()
            );
        }
        _ => {}
    }

    println!();
    println!("Supported shells: powershell, bash, zsh, fish, elvish");
    println!("To output raw script: agyo completion <shell> --raw");
}

/// Emit complete shell completion script including static clap AST and
/// dynamic Orbit argument completer hooks.
pub fn emit_completion_script(
    shell: Shell,
    cmd: &mut clap::Command,
    out: &mut dyn Write,
) -> std::io::Result<()> {
    // 1. Generate base static completion script from clap
    clap_complete::generate(shell, cmd, "agyo", out);

    // 2. Inject dynamic orbit argument completers
    match shell {
        Shell::PowerShell => {
            writeln!(
                out,
                r#"
# agyo dynamic orbit argument completer
Register-ArgumentCompleter -Native -CommandName 'agyo' -ScriptBlock {{
    param($wordToComplete, $commandAst, $cursorPosition)
    $elements = $commandAst.CommandElements
    if ($elements.Count -ge 2) {{
        $sub = $elements[1].Value
        if ($sub -in @('use', 'u', 'sw', 'remove', 'rm', 'quota', 'q', 'run', 'r')) {{
            if ($elements.Count -eq 2 -or ($elements.Count -eq 3 -and $cursorPosition -ge $elements[1].Extent.EndOffset)) {{
                $orbits = agyo __complete-orbits 2>$null
                $orbits | Where-Object {{ $_ -like "$wordToComplete*" }} | ForEach-Object {{
                    [System.Management.Automation.CompletionResult]::new($_, $_, 'ParameterValue', "Orbit: $_")
                }}
            }}
        }}
    }}
}}
"#
            )?;
        }
        Shell::Bash => {
            writeln!(
                out,
                r#"
# agyo dynamic orbit argument completer
_agyo_complete_wrapper() {{
    local cur prev words cword
    _init_completion || return
    local sub="${{words[1]}}"
    case "$sub" in
        use|u|sw|remove|rm|quota|q|run|r)
            if [ "$cword" -eq 2 ]; then
                local orbits=$(agyo __complete-orbits 2>/dev/null)
                COMPREPLY=( $(compgen -W "$orbits" -- "$cur") )
                return 0
            fi
            ;;
    esac
    _agyo "$@"
}}
complete -F _agyo_complete_wrapper -o bashdefault -o default agyo
"#
            )?;
        }
        Shell::Zsh => {
            writeln!(
                out,
                r#"
# agyo dynamic orbit argument completer
_agyo_zsh_wrapper() {{
    local -a words
    words=(${{(z)BUFFER}})
    if (( ${{#words}} >= 2 )); then
        case "${{words[2]}}" in
            use|u|sw|remove|rm|quota|q|run|r)
                if (( CURRENT == 2 || (CURRENT == 3 && ${{#words}} <= 3) )); then
                    local -a orbits
                    orbits=(${{(f)"$(agyo __complete-orbits 2>/dev/null)"}})
                    _describe -t orbits 'orbit' orbits
                    return 0
                fi
                ;;
        esac
    fi
    _agyo "$@"
}}
compdef _agyo_zsh_wrapper agyo
"#
            )?;
        }
        Shell::Fish => {
            writeln!(
                out,
                r#"
# agyo dynamic orbit argument completer
complete -c agyo -n '__fish_seen_subcommand_from use u sw remove rm quota q run r' -f -a '(agyo __complete-orbits 2>/dev/null)'
"#
            )?;
        }
        _ => {}
    }

    Ok(())
}

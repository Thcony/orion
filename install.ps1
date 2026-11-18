# Instalador do Orion. Uso no PowerShell:
#   irm https://raw.githubusercontent.com/Thcony/orion/main/install.ps1 | iex

$ErrorActionPreference = "Stop"
$Repo    = "Thcony/orion"
$Destino = if ($env:ORION_DIR) { $env:ORION_DIR } else { "$HOME\Orion" }
$PyVer   = "3.12.8"

Write-Host "`n=== Orion ===" -ForegroundColor Green

function Atualiza-Path {
    $env:Path = [Environment]::GetEnvironmentVariable("Path","Machine") + ";" +
                [Environment]::GetEnvironmentVariable("Path","User")
}

function Acha-Python {
    # comandos no PATH (ignorando o stub da Microsoft Store, que nao imprime versao)
    foreach ($c in @("python","python3","py")) {
        $cmd = Get-Command $c -ErrorAction SilentlyContinue
        if (-not $cmd) { continue }
        try {
            $v = & $cmd.Source --version 2>&1 | Out-String
            if ($v -match "Python 3\.(\d+)" -and [int]$Matches[1] -ge 9) { return $cmd.Source }
        } catch { }
    }
    # instalacoes conhecidas, caso o PATH nao tenha pegado
    foreach ($p in @("$env:LOCALAPPDATA\Programs\Python\Python3*\python.exe",
                     "$env:ProgramFiles\Python3*\python.exe",
                     "${env:ProgramFiles(x86)}\Python3*\python.exe",
                     "C:\Python3*\python.exe")) {
        $f = Get-ChildItem $p -ErrorAction SilentlyContinue |
             Sort-Object FullName -Descending | Select-Object -First 1
        if ($f) { return $f.FullName }
    }
    return $null
}

$py = Acha-Python

if (-not $py) {
    Write-Host "  Python nao achado. Instalando..." -ForegroundColor Yellow

    # tentativa 1: winget
    if (Get-Command winget -ErrorAction SilentlyContinue) {
        Write-Host "  via winget (aceita o prompt se aparecer)" -ForegroundColor Gray
        try {
            winget install -e --id Python.Python.3.12 --scope user `
                --accept-package-agreements --accept-source-agreements 2>&1 | Out-Null
        } catch { }
        Atualiza-Path
        $py = Acha-Python
    }

    # tentativa 2: instalador oficial do python.org, silencioso
    if (-not $py) {
        Write-Host "  via python.org ($PyVer)" -ForegroundColor Gray
        $arch = if ([Environment]::Is64BitOperatingSystem) { "amd64" } else { "win32" }
        $exe  = Join-Path $env:TEMP "python-$PyVer-$arch.exe"
        try {
            Invoke-WebRequest "https://www.python.org/ftp/python/$PyVer/python-$PyVer-$arch.exe" `
                -OutFile $exe -UseBasicParsing
            Start-Process $exe -Wait -ArgumentList @(
                "/quiet","InstallAllUsers=0","PrependPath=1","Include_pip=1","Include_test=0"
            )
            Remove-Item $exe -Force -ErrorAction SilentlyContinue
        } catch {
            Write-Host "  download falhou: $($_.Exception.Message)" -ForegroundColor Red
        }
        Atualiza-Path
        $py = Acha-Python
    }

    if (-not $py) {
        Write-Host "`n  Nao consegui instalar o Python sozinho." -ForegroundColor Red
        Write-Host "  Baixa manual: https://www.python.org/downloads/" -ForegroundColor Yellow
        Write-Host "  MARCA 'Add python.exe to PATH' na primeira tela, instala," -ForegroundColor Yellow
        Write-Host "  abre um PowerShell NOVO e roda esta linha de novo.`n" -ForegroundColor Yellow
        return
    }
    Write-Host "  Python instalado" -ForegroundColor Green
}

Write-Host "  $(& $py --version 2>&1)" -ForegroundColor Gray

# Baixa o Orion
$tmp = Join-Path $env:TEMP "orion-dl"
Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path $tmp | Out-Null
$zip = Join-Path $tmp "orion.zip"

Write-Host "  baixando o Orion..." -ForegroundColor Gray
$sha = $null; $msg = ""
try {
    $c = Invoke-RestMethod "https://api.github.com/repos/$Repo/commits/main" -Headers @{ "User-Agent" = "Orion-Installer" }
    $sha = $c.sha; $msg = ($c.commit.message -split "`n")[0]
} catch { }
$ref = if ($sha) { $sha } else { "refs/heads/main" }
Invoke-WebRequest "https://codeload.github.com/$Repo/zip/$ref" -OutFile $zip -UseBasicParsing
Expand-Archive $zip -DestinationPath $tmp -Force
$raiz = (Get-ChildItem $tmp -Directory | Select-Object -First 1).FullName
if (Test-Path (Join-Path $raiz "orion\main.py")) { $raiz = Join-Path $raiz "orion" }

# Copia preservando .env e data/
$novo = -not (Test-Path $Destino)
New-Item -ItemType Directory -Path $Destino -Force | Out-Null
Get-ChildItem $raiz -Recurse -File | ForEach-Object {
    $rel = $_.FullName.Substring($raiz.Length).TrimStart('\')
    if ($rel -eq ".env" -or $rel -like "data\*") { return }
    $alvo = Join-Path $Destino $rel
    New-Item -ItemType Directory -Path (Split-Path $alvo) -Force | Out-Null
    Copy-Item $_.FullName $alvo -Force
}
Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue

# Registra a versao instalada (senao o auto-update acha que ta desatualizado)
if ($sha) {
    New-Item -ItemType Directory -Path (Join-Path $Destino "data") -Force | Out-Null
    @{ sha = $sha; msg = $msg; quando = (Get-Date -Format s) } | ConvertTo-Json |
        Set-Content (Join-Path $Destino "data\versao.json") -Encoding UTF8
}

# Dependencia unica
Write-Host "  instalando psutil..." -ForegroundColor Gray
& $py -m pip install --quiet --disable-pip-version-check --user psutil 2>&1 | Out-Null

# .env
$envf = Join-Path $Destino ".env"
if (-not (Test-Path $envf)) {
    Copy-Item (Join-Path $Destino ".env.example") $envf
    Write-Host "`n  Chave gratis: https://aistudio.google.com/apikey" -ForegroundColor Yellow
    $chave = Read-Host "  Cola a chave do Gemini (ou Enter pra depois)"
    if ($chave) {
        (Get-Content $envf) -replace "cole_sua_chave_aqui", $chave.Trim() | Set-Content $envf
        Write-Host "  chave salva" -ForegroundColor Green
    }
}

# Atalho "orion" com o caminho do Python resolvido (nao depende do PATH)
$bat = Join-Path $Destino "orion.bat"
"@echo off`r`ncd /d `"$Destino`"`r`n`"$py`" main.py %*" | Set-Content $bat -Encoding ASCII
$userPath = [Environment]::GetEnvironmentVariable("Path","User")
if ($userPath -notlike "*$Destino*") {
    [Environment]::SetEnvironmentVariable("Path", "$userPath;$Destino", "User")
}

Write-Host "`n  Pronto em $Destino" -ForegroundColor Green
if ($novo) {
    Write-Host "  Abre um PowerShell NOVO e roda:  orion" -ForegroundColor Cyan
} else {
    Write-Host "  Atualizado. .env e memoria preservados." -ForegroundColor Cyan
}
Write-Host "  comandos: orion | orion atualizar | orion versao | orion reverter | orion modelos`n" -ForegroundColor Gray

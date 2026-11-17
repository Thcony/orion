# Instalador do Orion. Uso no PowerShell:
#   irm https://raw.githubusercontent.com/Thcony/orion/main/install.ps1 | iex

$ErrorActionPreference = "Stop"
$Repo    = "Thcony/orion"
$Destino = if ($env:ORION_DIR) { $env:ORION_DIR } else { "$HOME\Orion" }

Write-Host "`n=== Orion ===" -ForegroundColor Green

# Python
try {
    $v = (python --version 2>&1)
    Write-Host "  $v" -ForegroundColor Gray
} catch {
    Write-Host "  Python nao achado. Instala em python.org e marca 'Add to PATH'." -ForegroundColor Red
    return
}

# Baixa
$tmp = Join-Path $env:TEMP "orion-dl"
Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path $tmp | Out-Null
$zip = Join-Path $tmp "orion.zip"

Write-Host "  baixando..." -ForegroundColor Gray
Invoke-WebRequest "https://codeload.github.com/$Repo/zip/refs/heads/main" -OutFile $zip
Expand-Archive $zip -DestinationPath $tmp -Force
$raiz = (Get-ChildItem $tmp -Directory | Select-Object -First 1).FullName
if (Test-Path (Join-Path $raiz "orion\main.py")) { $raiz = Join-Path $raiz "orion" }

# Preserva .env e data/ se ja existir instalacao
$novo = -not (Test-Path $Destino)
New-Item -ItemType Directory -Path $Destino -Force | Out-Null
Get-ChildItem $raiz -Recurse -File | ForEach-Object {
    $rel = $_.FullName.Substring($raiz.Length).TrimStart('\')
    if ($rel -like ".env" -or $rel -like "data\*") { return }
    $alvo = Join-Path $Destino $rel
    New-Item -ItemType Directory -Path (Split-Path $alvo) -Force | Out-Null
    Copy-Item $_.FullName $alvo -Force
}
Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue

# Dependencia (uma so)
Write-Host "  instalando psutil..." -ForegroundColor Gray
python -m pip install --quiet --disable-pip-version-check psutil 2>&1 | Out-Null

# .env
$envf = Join-Path $Destino ".env"
if (-not (Test-Path $envf)) {
    Copy-Item (Join-Path $Destino ".env.example") $envf
    Write-Host "`n  Chave gratis: https://aistudio.google.com/apikey" -ForegroundColor Yellow
    $chave = Read-Host "  Cola a chave do Gemini (ou Enter pra depois)"
    if ($chave) {
        (Get-Content $envf) -replace "cole_sua_chave_aqui", $chave | Set-Content $envf
        Write-Host "  chave salva" -ForegroundColor Green
    }
}

# Atalho: digitar "orion" em qualquer lugar
$bat = Join-Path $Destino "orion.bat"
"@echo off`r`ncd /d `"$Destino`"`r`npython main.py %*" | Set-Content $bat -Encoding ASCII
$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($userPath -notlike "*$Destino*") {
    [Environment]::SetEnvironmentVariable("Path", "$userPath;$Destino", "User")
    Write-Host "  adicionei ao PATH (abre um terminal novo pra valer)" -ForegroundColor Gray
}

Write-Host "`n  Pronto em $Destino" -ForegroundColor Green
if ($novo) { Write-Host "  roda:  orion" -ForegroundColor Cyan }
else        { Write-Host "  atualizado, .env e memoria preservados" -ForegroundColor Cyan }
Write-Host "  comandos: orion | orion atualizar | orion versao | orion reverter | orion modelos`n" -ForegroundColor Gray

# Load environment variables from .env file
$envFile = Join-Path $PSScriptRoot "../.env"
if (Test-Path $envFile) {
    Get-Content $envFile | ForEach-Object {
        # Skip empty lines and comments
        if ($_ -and -not $_.StartsWith('#')) {
            $name, $value = $_.Split('=', 2)
            # Expand $PWD to current directory
            $value = $value.Trim('"').Replace('$PWD', $PWD.Path)
            [Environment]::SetEnvironmentVariable($name, $value, 'Process')
        }
    }
}

# Count pixels near a target RGB in an image (tolerance per channel).
# Usage: powershell -NoProfile -File count_color.ps1 -Image path.png -R 244 -G 116 -B 78 -Tol 24
param(
    [string]$Image,
    [int]$R,
    [int]$G,
    [int]$B,
    [int]$Tol = 24
)
Add-Type -AssemblyName System.Drawing
$bmp = [System.Drawing.Bitmap]::FromFile($Image)
$rect = New-Object System.Drawing.Rectangle 0, 0, $bmp.Width, $bmp.Height
$data = $bmp.LockBits($rect, [System.Drawing.Imaging.ImageLockMode]::ReadOnly, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
$bytes = New-Object byte[] ($data.Stride * $data.Height)
[System.Runtime.InteropServices.Marshal]::Copy($data.Scan0, $bytes, 0, $bytes.Length)
$bmp.UnlockBits($data)
$bmp.Dispose()

$count = 0
$step = 4  # BGRA
for ($i = 0; $i -lt $bytes.Length; $i += $step) {
    $b = $bytes[$i]; $g = $bytes[$i + 1]; $r = $bytes[$i + 2]
    if ([Math]::Abs($r - $R) -le $Tol -and [Math]::Abs($g - $G) -le $Tol -and [Math]::Abs($b - $B) -le $Tol) {
        $count++
    }
}
Write-Output "matched: $count"

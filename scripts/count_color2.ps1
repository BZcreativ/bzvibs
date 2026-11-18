# Count pixels near a target RGB using strided GetPixel sampling.
# Usage: powershell -NoProfile -File count_color2.ps1 -Image path.png -R 244 -G 116 -B 78 -Tol 24 [-Stride 3]
param(
    [string]$Image,
    [int]$R,
    [int]$G,
    [int]$B,
    [int]$Tol = 24,
    [int]$Stride = 3
)
Add-Type -AssemblyName System.Drawing
$bmp = [System.Drawing.Bitmap]::FromFile($Image)
$count = 0
$samples = 0
for ($x = 0; $x -lt $bmp.Width; $x += $Stride) {
    for ($y = 0; $y -lt $bmp.Height; $y += $Stride) {
        $c = $bmp.GetPixel($x, $y)
        if ([Math]::Abs([int]$c.R - $R) -le $Tol -and
            [Math]::Abs([int]$c.G - $G) -le $Tol -and
            [Math]::Abs([int]$c.B - $B) -le $Tol) {
            $count++
        }
        $samples++
    }
}
$bmp.Dispose()
Write-Output "matched: $count of $samples sampled"

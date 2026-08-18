# Generates the "DeformationLab" demo Space for testing runtime deformation
# and fracture.
#
#   pwsh -File eustress/tools/make_deformation_demo.ps1
#
# Creates <Documents>\Eustress\Universe1\Spaces\DeformationLab with three test
# stations. Re-running overwrites the Space's Workspace cleanly.
#
# WHY THE NUMBERS ARE WHAT THEY ARE
# ---------------------------------
# Dent depth and crack threshold are computed from real contact mechanics, not
# tuned constants, so the demo only shows anything if the materials are chosen
# to put the impact energy on the right side of each threshold:
#
#   impact energy      E   = J^2 / 2*m_eff        (J = solved contact impulse)
#   fracture threshold E_f = (K_IC^2 / E_young) * smallest_cross_section
#   plastic dent depth d   = sqrt((E - E_y) / (pi * R * 3*yield_strength))
#
# Dropper: 0.8 m cube, density 2000 -> ~1024 kg, falling ~9 m -> ~13 m/s,
# giving E ~ 85 kJ. Against a 6 x 0.6 x 4 plate the smallest cross-section is
# 0.6 * 4 = 2.4 m^2. So:
#
#   Station A (DENT)     soft + TOUGH: K_IC 5e6 over E_young 1e8
#                        -> E_f = 6.0e5 J  >> 85 kJ  -> never cracks
#                        yield 5e5 -> dent ~0.19 m, clamped to 25% of the
#                        plate's 0.6 m thickness = 0.15 m. Clearly visible.
#   Station B (FRACTURE) concrete-like: K_IC 1e6 over E_young 3e10
#                        -> E_f = 80 J  <<  85 kJ  -> cracks in half
#   Station C (CONTROL)  deformation = false -> no DeformableMesh at all, so
#                        it neither dents nor cracks. This station is the one
#                        that can FAIL the test: if C reacts, the opt-in gate
#                        is broken.

$ErrorActionPreference = 'Stop'

# NOTE: do NOT use [Environment]::GetFolderPath('MyDocuments') here. On this
# machine that resolves to the OneDrive-redirected folder
# (…\OneDrive\Documentos\…), which is NOT where the engine looks — the engine
# reports its root as <UserProfile>\Documents\Eustress. Writing to the
# redirected path silently produces a Space the engine never sees.
$Root = Join-Path $env:USERPROFILE 'Documents\Eustress'
if (-not (Test-Path $Root)) {
    throw "Eustress root not found at $Root — pass the correct root or create it first."
}
$Space = Join-Path $Root 'Universe1\Spaces\DeformationLab'
$Ws    = Join-Path $Space 'Workspace'

if (Test-Path $Ws) { Remove-Item $Ws -Recurse -Force }
New-Item -ItemType Directory -Force -Path $Ws | Out-Null

$stamp = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")

# Every numeric array MUST emit as TOML floats. PowerShell renders 0.0 as "0",
# which TOML parses as an INTEGER — and serde refuses to deserialize an integer
# into f32, so `position = [0, 1, 0]` fails the whole part at load. The color
# field is worse than a hard error: its flexible parser treats an integer array
# as 0-255 RGB, so `[0.72, 0.72, 0.7, 1]` would silently be read as a
# near-black colour instead of light grey.
function Fmt-Vec {
    param([float[]]$V)
    ($V | ForEach-Object { '{0:0.0######}' -f $_ }) -join ', '
}

function New-Part {
    param(
        [string]$Name,
        [float[]]$Pos,
        [float[]]$Size,
        [float[]]$Color,
        [bool]$Anchored,
        [bool]$Deformation = $false,
        [string]$MaterialName = 'Plastic',
        [float]$Density = 900,
        [hashtable]$Material = $null,
        [float]$PhysicsDensity = 0
    )

    $dir = Join-Path $Ws $Name
    New-Item -ItemType Directory -Force -Path $dir | Out-Null

    $sb = [System.Text.StringBuilder]::new()
    [void]$sb.AppendLine('[asset]')
    [void]$sb.AppendLine('mesh = "parts/block.glb"')
    [void]$sb.AppendLine('scene = "Scene0"')
    [void]$sb.AppendLine('')
    [void]$sb.AppendLine('[metadata]')
    [void]$sb.AppendLine('class_name = "Part"')
    [void]$sb.AppendLine("name = `"$Name`"")
    [void]$sb.AppendLine('archivable = true')
    [void]$sb.AppendLine("created = `"$stamp`"")
    [void]$sb.AppendLine("last_modified = `"$stamp`"")
    [void]$sb.AppendLine("uuid = `"$([guid]::NewGuid().ToString('N'))`"")
    [void]$sb.AppendLine('')
    [void]$sb.AppendLine('[properties]')
    [void]$sb.AppendLine("color = [$(Fmt-Vec $Color)]")
    [void]$sb.AppendLine("anchored = $($Anchored.ToString().ToLower())")
    [void]$sb.AppendLine('can_collide = true')
    [void]$sb.AppendLine('cast_shadow = true')
    [void]$sb.AppendLine('locked = false')
    [void]$sb.AppendLine('transparency = 0.0')
    [void]$sb.AppendLine('reflectance = 0.05')
    [void]$sb.AppendLine("material = `"$MaterialName`"")
    # The opt-in that makes the whole vertex-deformation pipeline consider this
    # part at all. Without it there is no DeformableMesh and nothing happens.
    [void]$sb.AppendLine("destructible = $($Deformation.ToString().ToLower())")
    [void]$sb.AppendLine('')

    if ($PhysicsDensity -gt 0) {
        [void]$sb.AppendLine('[properties.physics]')
        [void]$sb.AppendLine("density = $PhysicsDensity")
        [void]$sb.AppendLine('friction_static = 0.6')
        [void]$sb.AppendLine('friction_kinetic = 0.5')
        [void]$sb.AppendLine('restitution = 0.05')
        [void]$sb.AppendLine('')
    }

    if ($Material) {
        # Full MaterialProperties. `fracture_toughness` (K_IC) and
        # `yield_strength` are what decide crack-vs-dent.
        [void]$sb.AppendLine('[material]')
        foreach ($k in $Material.Keys) {
            [void]$sb.AppendLine("$k = $($Material[$k])")
        }
        [void]$sb.AppendLine('')
    }

    [void]$sb.AppendLine('[transform]')
    [void]$sb.AppendLine("position = [$(Fmt-Vec $Pos)]")
    [void]$sb.AppendLine('rotation = [0.0, 0.0, 0.0, 1.0]')
    [void]$sb.AppendLine("scale = [$(Fmt-Vec $Size)]")

    $file = Join-Path $dir '_instance.toml'
    [System.IO.File]::WriteAllText($file, $sb.ToString())
}

# ---------------------------------------------------------------- baseplate
New-Part -Name 'Baseplate' -Pos @(0.0, -0.5, 0.0) -Size @(80.0, 1.0, 40.0) `
    -Color @(0.16, 0.17, 0.19, 1.0) -Anchored $true -MaterialName 'Concrete'

# ------------------------------------------------- Station A — DENT (no crack)
# Deliberately SMALLER than the fracture plate. Dent depth comes out of contact
# mechanics (~0.15 m here) and the crater radius is ~0.4 m; on a 6 m slab that
# is a dimple you have to hunt for. On a 3 m plate it reads as real damage, and
# the subdivision (which targets a world-space edge length) resolves it with
# roughly twice the vertex density for the same triangle budget.
New-Part -Name 'A_DentPlate_Soft' -Pos @(0.0, 1.0, 0.0) -Size @(3.0, 0.5, 2.5) `
    -Color @(0.85, 0.62, 0.20, 1.0) -Anchored $true -Deformation $true `
    -MaterialName 'Metal' -Material @{
        name               = '"SoftAlloy"'
        young_modulus      = '1.0e8'   # very compliant -> big visible dent
        poisson_ratio      = '0.33'
        # Dent depth is d = sqrt(E / (pi * R * 3*yield_strength)), so yield
        # strength is the dial that decides whether the crater is visible.
        # 5.0e5 (a soft alloy) gave a physically correct but ~1 cm dimple on
        # this ~1 kJ impact. 5.0e3 is putty/soft-lead territory and drives the
        # dent to the 25%-of-thickness clamp, i.e. obvious damage.
        # NOTE this does NOT risk cracking: the fracture threshold is
        # G_c = K_IC^2 / E_young times cross-section, and depends on neither
        # yield strength nor this change.
        yield_strength     = '5.0e3'
        ultimate_strength  = '8.0e3'
        fracture_toughness = '5.0e6'   # VERY tough -> E_f ~600 kJ, never cracks
        hardness           = '30.0'
        density            = '2700.0'
    }

New-Part -Name 'A_Dropper' -Pos @(0.0, 10.0, 0.0) -Size @(0.8, 0.8, 0.8) `
    -Color @(0.80, 0.15, 0.15, 1.0) -Anchored $false -MaterialName 'Metal' `
    -Density 2000 -PhysicsDensity 2000

# ------------------------------------------------------ Station B — FRACTURE
New-Part -Name 'B_FracturePlate_Concrete' -Pos @(-14.0, 1.0, 0.0) -Size @(6.0, 0.6, 4.0) `
    -Color @(0.72, 0.72, 0.70, 1.0) -Anchored $true -Deformation $true `
    -MaterialName 'Concrete' -Density 2400 -Material @{
        name               = '"Concrete"'
        young_modulus      = '3.0e10'
        poisson_ratio      = '0.20'
        yield_strength     = '3.0e7'
        ultimate_strength  = '3.0e7'
        fracture_toughness = '1.0e6'   # brittle -> E_f ~80 J, cracks in half
        hardness           = '500.0'
        density            = '2400.0'
    }

New-Part -Name 'B_Dropper' -Pos @(-14.0, 10.0, 0.0) -Size @(0.8, 0.8, 0.8) `
    -Color @(0.80, 0.15, 0.15, 1.0) -Anchored $false -MaterialName 'Metal' `
    -Density 2000 -PhysicsDensity 2000

# ------------------------------------------- Station C — CONTROL (must NOT react)
New-Part -Name 'C_ControlPlate_Rigid' -Pos @(14.0, 1.0, 0.0) -Size @(6.0, 0.6, 4.0) `
    -Color @(0.35, 0.45, 0.75, 1.0) -Anchored $true -Deformation $false `
    -MaterialName 'Concrete' -Density 2400

New-Part -Name 'C_Dropper' -Pos @(14.0, 10.0, 0.0) -Size @(0.8, 0.8, 0.8) `
    -Color @(0.80, 0.15, 0.15, 1.0) -Anchored $false -MaterialName 'Metal' `
    -Density 2000 -PhysicsDensity 2000

Write-Host "Demo Space written to: $Space"
Get-ChildItem $Ws -Directory | ForEach-Object { Write-Host "  - $($_.Name)" }

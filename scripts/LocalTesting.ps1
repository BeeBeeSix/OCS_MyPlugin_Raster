
#TESTING
   Set-Location (Split-Path $PSScriptRoot -Parent)
   cargo build --release
   $d = "$env:APPDATA\OpenCADStudio\plugins\opencad.rastertools" 
   mkdir $d -Force
   copy target\release\opencad_raster_plugin.dll $d
   copy plugin.toml $d
   Pause

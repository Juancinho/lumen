# Run with Windows PowerShell 5.1 (System.Drawing is a built-in test dependency only).
param([Parameter(Mandatory=$true)][string]$Prefix)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @'
using System.Drawing;
using System.Drawing.Imaging;
using System.Runtime.InteropServices;
public static class LumenOcrFixture {
 public static void Save(string prefix) {
  using(var b=new Bitmap(1200,300,PixelFormat.Format24bppRgb)) {
   using(var g=Graphics.FromImage(b)) using(var f=new Font("Arial",38)) {
    g.Clear(Color.White); g.DrawString("ERROR 42",f,Brushes.Black,35,35);
    g.DrawString("Buscar texto en imagenes",f,Brushes.Black,35,120);
   }
   b.Save(prefix+".png",ImageFormat.Png);
   var d=b.LockBits(new Rectangle(0,0,1200,300),ImageLockMode.ReadOnly,PixelFormat.Format24bppRgb);
   var raw=new byte[d.Stride*300]; Marshal.Copy(d.Scan0,raw,0,raw.Length); b.UnlockBits(d);
   var rgb=new byte[1200*300*3];
   for(int y=0;y<300;y++) for(int x=0;x<1200;x++) {
    int a=y*d.Stride+x*3; int z=(y*1200+x)*3;
    rgb[z]=raw[a+2];rgb[z+1]=raw[a+1];rgb[z+2]=raw[a];
   }
   System.IO.File.WriteAllBytes(prefix+".rgb",rgb);
  }
 }
}
'@
[LumenOcrFixture]::Save([System.IO.Path]::GetFullPath($Prefix))

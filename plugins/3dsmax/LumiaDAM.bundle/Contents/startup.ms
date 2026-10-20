-- Python modülü eklenti içinden yüklenir; kütüphane dosyaları betik olarak çalıştırılmaz.
try
(
    local bridgeFile = (getFilenamePath (getSourceFileName())) + "lumiadam_max.py"
    local runner = python.Import "runpy"
    runner.run_path bridgeFile run_name:"__main__"
)
catch (format "LumiaDAM Bridge startup failed: %\n" (getCurrentException()))

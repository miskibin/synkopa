Linux builds bundle Microsoft's ONNX Runtime here (libonnxruntime.so.1, MIT): run
node scripts/fetch-onnxruntime.mjs before tauri build. Synkopa loads it at run time for the
RMVPE pitch model; without it Synkopa uses its built-in YIN detector.

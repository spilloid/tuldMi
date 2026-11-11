tuldMi third-party licenses
===========================

tuldMi itself is MIT-licensed (LICENSE in the source repository,
https://github.com/spilloid/tuldMi). It ships the third-party components
below, each under its own license. This folder holds those license texts.
THIRD_PARTY_NOTICES.md, beside it, lists every compiled-in Rust crate and
bundled npm package with its license and repository, and
third-party-licenses.txt holds the license, copyright and notice files those
packages publish.

Component                        License        Text in this folder
-------------------------------  -------------  ---------------------------------
Shipped inside the app or its helpers (compiled in):

whisper.cpp / ggml               MIT            whisper.cpp-LICENSE.txt
libopus                          BSD-3-Clause   opus-COPYING.txt
SQLite                           Public domain  (no license text needed)
sherpa-onnx (tuldmi-speakers)    Apache-2.0     Apache-2.0.txt
ONNX Runtime 1.28.2              MIT            onnxruntime-LICENSE.txt, and the
                                                libraries it bundles:
                                                onnxruntime-ThirdPartyNotices.txt
kaldi-native-fbank               Apache-2.0     Apache-2.0.txt
kaldi-decoder                    Apache-2.0     Apache-2.0.txt
kaldifst / OpenFst               Apache-2.0     kaldifst-and-OpenFst-LICENSE.txt
simple-sentencepiece             Apache-2.0     Apache-2.0.txt
hclust-cpp (fastcluster)         BSD-2-Clause   hclust-cpp-LICENSE.txt
Darts-clone                      BSD-2-Clause   darts-clone-LICENSE.txt
KISS FFT                         BSD-3-Clause   kissfft-LICENSE.txt
Rust crates, npm packages        (various)      third-party-licenses.txt

Shipped models:

Silero VAD v6.2.0                MIT            silero-vad-LICENSE.txt
  (Silero Team; ggml conversion by ggml-org)
CED-tiny AudioSet tagger         Apache-2.0     Apache-2.0.txt
  (RicherMans/CED; ONNX int8 export by the sherpa-onnx project;
  https://huggingface.co/mispeech/ced-tiny)
  audioset-labels.csv, its class list, comes with that model package;
  the class names are from Google's AudioSet ontology
  (https://research.google.com/audioset/).

Each sherpa-onnx, kaldi-native-fbank, kaldi-decoder and simple-sentencepiece
release carries the stock Apache License 2.0 text unchanged, with copyright
held by their respective authors (see each project's repository):
  https://github.com/k2-fsa/sherpa-onnx (v1.13.8)
  https://github.com/csukuangfj/kaldi-native-fbank
  https://github.com/k2-fsa/kaldi-decoder
  https://github.com/k2-fsa/kaldifst
  https://github.com/pkufool/simple-sentencepiece (v0.7)
None of them ships a NOTICE file.

Models you download from inside the app (Whisper; the speaker models) are
listed with their licenses in Settings and in docs/MODELS.md.

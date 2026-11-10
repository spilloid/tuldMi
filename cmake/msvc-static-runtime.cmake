# Handed to every CMake build in the workspace (CMAKE_TOOLCHAIN_FILE in .cargo/config.toml).
# On Windows the workspace links the static C runtime (+crt-static, needed by sherpa-onnx's /MT
# libraries). Projects that pick the runtime through CMake rather than cmake-rs's -MT flag must
# be told too, or their objects import the DLL runtime (LNK4217) into a static-runtime binary:
#   - CMAKE_MSVC_RUNTIME_LIBRARY, for projects with policy CMP0091 NEW;
#   - OPUS_STATIC_RUNTIME, libopus's own switch (opusic-sys), which otherwise forces /MD.
# Elsewhere this file does nothing.
if(CMAKE_HOST_WIN32 OR CMAKE_HOST_SYSTEM_NAME STREQUAL "Windows")
  set(CMAKE_MSVC_RUNTIME_LIBRARY "MultiThreaded" CACHE STRING "Static C runtime, matching +crt-static")
  set(OPUS_STATIC_RUNTIME ON CACHE BOOL "Static C runtime, matching +crt-static")
endif()

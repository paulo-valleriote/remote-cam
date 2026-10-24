# DirectShow fallback virtual camera (T034).
#
# **NON-PRODUCTION** — F0 risk-reduction spike. Project-owned code.
#
# Two builds, one per bitness, because a 32-bit consumer on 64-bit Windows can
# only load a 32-bit DLL and reads the WOW6432Node registry view. The CMake
# target names differ so a build directory can never be reused for the other
# bitness by accident.
#
# Not built on the F0 host (no MSVC, no Windows SDK); see README.md.

cmake_minimum_required(VERSION 3.24)
project(remotecam_dshow_vcam_fallback CXX)

if(NOT MSVC)
  message(FATAL_ERROR "the DirectShow fallback needs the MSVC toolchain; the F0 host has none")
endif()

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

if(CMAKE_SIZEOF_VOID_P EQUAL 8)
  set(REMOTECAM_BITNESS 64)
else()
  set(REMOTECAM_BITNESS 32)
endif()

message(STATUS "building the RemoteCam DirectShow fallback for ${REMOTECAM_BITNESS}-bit consumers")

add_library(dshow_vcam_fallback SHARED
  src/dshow_vcam_fallback.cpp
  src/registry.cpp
  src/test_pattern.cpp
  src/dshow_vcam_fallback.def
)

target_include_directories(dshow_vcam_fallback PRIVATE include)

# strmiids.lib carries the DirectShow GUIDs; quartz.lib carries the capture
# category; winmm.lib carries the timing functions the live pin uses.
target_link_libraries(dshow_vcam_fallback PRIVATE strmiids quartz winmm ole32 oleaut32 uuid)

# The DirectShow base classes (streams.h) are not part of the SDK; they ship as
# samples. Point this at the samples' BaseClasses directory on the build machine
# rather than vendoring them, because the samples' licence is not this project's.
set(REMOTECAM_BASE_CLASSES "" CACHE PATH "Path to the DirectShow BaseClasses directory")
if(NOT REMOTECAM_BASE_CLASSES)
  message(FATAL_ERROR
    "set -DREMOTECAM_BASE_CLASSES=<path to samples\\multimedia\\directshow\\baseclasses>")
endif()

add_subdirectory(${REMOTECAM_BASE_CLASSES} ${CMAKE_CURRENT_BINARY_DIR}/baseclasses EXCLUDE_FROM_ALL)
target_include_directories(dshow_vcam_fallback PRIVATE ${REMOTECAM_BASE_CLASSES})
# The base classes ship a debug and a release library; linking the wrong one
# produces link errors that look like missing symbols.
target_link_libraries(dshow_vcam_fallback PRIVATE
  $<$<CONFIG:Debug>:strmbasd>
  $<$<NOT:$<CONFIG:Debug>>:strmbase>
)

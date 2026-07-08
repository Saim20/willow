cmake_minimum_required(VERSION 3.16)
project(willow-service VERSION 3.0.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

option(BUILD_SHARED_LIBS "Build shared libraries" OFF)

find_package(PkgConfig REQUIRED)
find_package(Threads REQUIRED)

pkg_check_modules(SDBUS REQUIRED IMPORTED_TARGET sdbus-c++)
pkg_check_modules(JSONCPP REQUIRED IMPORTED_TARGET jsoncpp)
pkg_check_modules(PULSEAUDIO IMPORTED_TARGET libpulse-simple)

# sherpa-onnx via FetchContent
set(SHERPA_ONNX_GIT_TAG "v1.12.9" CACHE STRING "sherpa-onnx tag")
set(SHERPA_ONNX_ENABLE_PYTHON OFF CACHE BOOL "" FORCE)
set(SHERPA_ONNX_ENABLE_TESTS OFF CACHE BOOL "" FORCE)
set(SHERPA_ONNX_ENABLE_CHECK OFF CACHE BOOL "" FORCE)
set(SHERPA_ONNX_ENABLE_PORTAUDIO OFF CACHE BOOL "" FORCE)
set(SHERPA_ONNX_ENABLE_BINARY OFF CACHE BOOL "" FORCE)
set(SHERPA_ONNX_ENABLE_JNI OFF CACHE BOOL "" FORCE)
set(SHERPA_ONNX_ENABLE_C_API ON CACHE BOOL "" FORCE)
set(SHERPA_ONNX_ENABLE_WEBSOCKET OFF CACHE BOOL "" FORCE)
set(SHERPA_ONNX_ENABLE_TTS OFF CACHE BOOL "" FORCE)
set(SHERPA_ONNX_ENABLE_SPEAKER_DIARIZATION ON CACHE BOOL "" FORCE)

include(FetchContent)
FetchContent_Declare(
    sherpa-onnx
    GIT_REPOSITORY https://github.com/k2-fsa/sherpa-onnx.git
    GIT_TAG        ${SHERPA_ONNX_GIT_TAG}
    GIT_SHALLOW    TRUE
)
# openfst in sherpa-onnx fails with C++20 on GCC 16; build sherpa with C++17
set(_WILLOW_SAVED_CXX_STANDARD ${CMAKE_CXX_STANDARD})
set(CMAKE_CXX_STANDARD 17)
FetchContent_MakeAvailable(sherpa-onnx)
set(CMAKE_CXX_STANDARD ${_WILLOW_SAVED_CXX_STANDARD})

set(SOURCES
    src/main.cpp
    src/VoiceAssistantService.cpp
    src/CommandExecutor.cpp
    src/ModeWorkers.cpp
    src/ModelPaths.cpp
    src/AudioRouter.cpp
    src/KeywordSpotterEngine.cpp
    src/StreamingASREngine.cpp
    src/SpeakerVerifier.cpp
    src/CommandPhraseIndex.cpp
    src/CommandIntentResolver.cpp
    src/TypingStreamWriter.cpp
    src/TtsEngine.cpp
    src/SpeechPipeline.cpp
)

set(HEADERS
    src/VoiceAssistantService.hpp
    src/CommandExecutor.hpp
    src/ModeWorkers.hpp
    src/ModelPaths.hpp
    src/AudioRouter.hpp
    src/KeywordSpotterEngine.hpp
    src/StreamingASREngine.hpp
    src/SpeakerVerifier.hpp
    src/CommandPhraseIndex.hpp
    src/CommandIntentResolver.hpp
    src/TypingStreamWriter.hpp
    src/TtsEngine.hpp
    src/SpeechPipeline.hpp
)

add_executable(willow-service ${SOURCES} ${HEADERS})

target_link_libraries(willow-service
    PRIVATE
        PkgConfig::SDBUS
        PkgConfig::JSONCPP
        sherpa-onnx-cxx-api
        sherpa-onnx-c-api
        Threads::Threads
)

if(PULSEAUDIO_FOUND)
    target_link_libraries(willow-service PRIVATE PkgConfig::PULSEAUDIO)
    target_compile_definitions(willow-service PRIVATE HAVE_PULSEAUDIO)
endif()

target_include_directories(willow-service
    PRIVATE
        ${CMAKE_CURRENT_SOURCE_DIR}/src
        ${sherpa-onnx_SOURCE_DIR}
)

target_compile_options(willow-service
    PRIVATE
        -Wall
        -Wextra
        -Wpedantic
        $<$<CONFIG:Debug>:-g -O0>
        $<$<CONFIG:Release>:-O3>
)

install(TARGETS willow-service
    RUNTIME DESTINATION bin
    COMPONENT willow
)

configure_file(
    ${CMAKE_CURRENT_SOURCE_DIR}/../dbus/com.github.saim.Willow.service.in
    ${CMAKE_CURRENT_BINARY_DIR}/com.github.saim.Willow.service
    @ONLY
)

install(FILES ${CMAKE_CURRENT_BINARY_DIR}/com.github.saim.Willow.service
    DESTINATION share/dbus-1/services
    COMPONENT willow
)

install(FILES ${CMAKE_CURRENT_SOURCE_DIR}/../dbus/com.github.saim.Willow.xml
    DESTINATION share/dbus-1/interfaces
    COMPONENT willow
)

message(STATUS "")
message(STATUS "Willow Service Configuration:")
message(STATUS "  Version: ${PROJECT_VERSION}")
message(STATUS "  C++ Standard: ${CMAKE_CXX_STANDARD}")
message(STATUS "")

cmake_minimum_required(VERSION 3.16)
project(willow-service VERSION 2.0.0 LANGUAGES CXX)

# sdbus-c++ 2.x requires C++20
set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

option(BUILD_SHARED_LIBS "Build shared libraries" OFF)

# Auto-detect CUDA
find_package(CUDAToolkit QUIET)
if(CUDAToolkit_FOUND)
    option(GGML_CUDA "Enable CUDA support" ON)
    message(STATUS "CUDA Toolkit found - enabling CUDA support")
else()
    option(GGML_CUDA "Enable CUDA support" OFF)
    message(STATUS "CUDA Toolkit not found - CUDA support disabled")
endif()

# Auto-detect Vulkan
find_package(Vulkan QUIET)
if(Vulkan_FOUND)
    option(GGML_VULKAN "Enable Vulkan support" ON)
    message(STATUS "Vulkan SDK found - enabling Vulkan support")
else()
    option(GGML_VULKAN "Enable Vulkan support" OFF)
    message(STATUS "Vulkan SDK not found - Vulkan support disabled")
endif()

find_package(PkgConfig REQUIRED)
find_package(Threads REQUIRED)

pkg_check_modules(SDBUS REQUIRED IMPORTED_TARGET sdbus-c++)
pkg_check_modules(JSONCPP REQUIRED IMPORTED_TARGET jsoncpp)
pkg_check_modules(PULSEAUDIO IMPORTED_TARGET libpulse-simple)

# whisper.cpp: prefer the git submodule, otherwise fetch a pinned release
set(WHISPER_CPP_DIR "${CMAKE_SOURCE_DIR}/../whisper.cpp" CACHE PATH "Path to whisper.cpp")
set(WHISPER_CPP_GIT_TAG "v1.9.1" CACHE STRING "whisper.cpp tag when fetching via FetchContent")

set(GGML_CUDA ${GGML_CUDA} CACHE BOOL "Enable CUDA" FORCE)
set(GGML_VULKAN ${GGML_VULKAN} CACHE BOOL "Enable Vulkan" FORCE)
set(WHISPER_BUILD_EXAMPLES OFF CACHE BOOL "Disable whisper examples" FORCE)
set(WHISPER_BUILD_TESTS OFF CACHE BOOL "Disable whisper tests" FORCE)
set(WHISPER_BUILD_SERVER OFF CACHE BOOL "Disable whisper server" FORCE)

if(EXISTS "${WHISPER_CPP_DIR}/CMakeLists.txt")
    message(STATUS "Using whisper.cpp from: ${WHISPER_CPP_DIR}")
    add_subdirectory(${WHISPER_CPP_DIR} whisper.cpp EXCLUDE_FROM_ALL)
    set(WHISPER_INCLUDE_DIRS
        "${WHISPER_CPP_DIR}/include"
        "${WHISPER_CPP_DIR}/ggml/include"
        "${WHISPER_CPP_DIR}"
    )
else()
    message(STATUS "whisper.cpp not found locally; fetching ${WHISPER_CPP_GIT_TAG} via FetchContent")
    include(FetchContent)
    FetchContent_Declare(
        whisper
        GIT_REPOSITORY https://github.com/ggml-org/whisper.cpp
        GIT_TAG        ${WHISPER_CPP_GIT_TAG}
    )
    FetchContent_MakeAvailable(whisper)
    set(WHISPER_INCLUDE_DIRS
        "${whisper_SOURCE_DIR}/include"
        "${whisper_SOURCE_DIR}/ggml/include"
        "${whisper_SOURCE_DIR}"
    )
endif()

set(SOURCES
    src/main.cpp
    src/VoiceAssistantService.cpp
    src/CommandExecutor.cpp
    src/SpeechSegmenter.cpp
    src/ModeWorkers.cpp
)

set(HEADERS
    src/VoiceAssistantService.hpp
    src/CommandExecutor.hpp
    src/SpeechSegmenter.hpp
    src/ModeWorkers.hpp
)

add_executable(willow-service ${SOURCES} ${HEADERS})

target_link_libraries(willow-service
    PRIVATE
        PkgConfig::SDBUS
        PkgConfig::JSONCPP
        whisper
        Threads::Threads
)

if(PULSEAUDIO_FOUND)
    target_link_libraries(willow-service PRIVATE PkgConfig::PULSEAUDIO)
    target_compile_definitions(willow-service PRIVATE HAVE_PULSEAUDIO)
endif()

target_include_directories(willow-service
    PRIVATE
        ${CMAKE_CURRENT_SOURCE_DIR}/src
        ${WHISPER_INCLUDE_DIRS}
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
)

configure_file(
    ${CMAKE_CURRENT_SOURCE_DIR}/../dbus/com.github.saim.Willow.service.in
    ${CMAKE_CURRENT_BINARY_DIR}/com.github.saim.Willow.service
    @ONLY
)

install(FILES ${CMAKE_CURRENT_BINARY_DIR}/com.github.saim.Willow.service
    DESTINATION share/dbus-1/services
)

install(FILES ${CMAKE_CURRENT_SOURCE_DIR}/../dbus/com.github.saim.Willow.xml
    DESTINATION share/dbus-1/interfaces
)

message(STATUS "")
message(STATUS "Voice Assistant Service Configuration:")
message(STATUS "  Version: ${PROJECT_VERSION}")
message(STATUS "  C++ Standard: ${CMAKE_CXX_STANDARD}")
message(STATUS "  Build Type: ${CMAKE_BUILD_TYPE}")
message(STATUS "  Install Prefix: ${CMAKE_INSTALL_PREFIX}")
message(STATUS "  CUDA Support: ${GGML_CUDA}")
message(STATUS "  Vulkan Support: ${GGML_VULKAN}")
message(STATUS "")

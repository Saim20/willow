#include "TtsEngine.hpp"
#include <iostream>
#include <spawn.h>
#include <unistd.h>
#include <sys/wait.h>

extern char **environ;

namespace VoiceAssistant {

namespace {

int spawnProcess(const std::vector<std::string>& args) {
    if (args.empty()) return -1;
    std::vector<char*> argv;
    argv.reserve(args.size() + 1);
    for (const auto& arg : args) {
        argv.push_back(const_cast<char*>(arg.c_str()));
    }
    argv.push_back(nullptr);

    pid_t pid = 0;
    if (posix_spawnp(&pid, argv[0], nullptr, nullptr, argv.data(), environ) != 0) {
        return -1;
    }
    int status = 0;
    if (waitpid(pid, &status, 0) < 0) return -1;
    return WIFEXITED(status) ? WEXITSTATUS(status) : -1;
}

bool commandExists(const char* cmd) {
    return spawnProcess({"/bin/sh", "-c", std::string("which ") + cmd + " >/dev/null 2>&1"}) == 0;
}

} // namespace

TtsEngine::TtsEngine() = default;

TtsEngine::~TtsEngine() {
    shutdown();
}

void TtsEngine::log(const std::string& level, const std::string& message) {
    std::cout << "[TtsEngine] [" << level << "] " << message << std::endl;
}

bool TtsEngine::initialize(const ModelPaths& paths) {
    shutdown();
    (void)paths;
    // Use system TTS (spd-say or espeak) since sherpa TTS is disabled in build
    if (commandExists("spd-say")) {
        m_loaded = true;
        log("INFO", "TTS engine initialized (spd-say)");
        return true;
    }
    if (commandExists("espeak")) {
        m_loaded = true;
        log("INFO", "TTS engine initialized (espeak)");
        return true;
    }
    log("WARNING", "No TTS backend found (install speech-dispatcher or espeak)");
    return false;
}

void TtsEngine::shutdown() {
    m_stop = true;
    m_cv.notify_all();
    if (m_worker.joinable()) {
        m_worker.join();
    }
    m_loaded = false;
}

void TtsEngine::speakAsync(const std::string& text) {
    if (!m_config.enabled || text.empty()) return;
    {
        std::lock_guard<std::mutex> lock(m_mutex);
        m_queue.push(text);
    }
    if (!m_worker.joinable()) {
        m_stop = false;
        m_worker = std::thread(&TtsEngine::workerLoop, this);
    }
    m_cv.notify_one();
}

void TtsEngine::speak(const std::string& text) {
    speakAsync(text);
}

void TtsEngine::playAudio(const std::vector<float>&, int) {
    // Not used with system TTS
}

void TtsEngine::workerLoop() {
    while (!m_stop) {
        std::string text;
        {
            std::unique_lock<std::mutex> lock(m_mutex);
            m_cv.wait(lock, [this] { return m_stop || !m_queue.empty(); });
            if (m_stop) break;
            text = m_queue.front();
            m_queue.pop();
        }

        if (!m_loaded || text.empty()) continue;
        if (m_callback) m_callback(text, true);

        int result = -1;
        if (commandExists("spd-say")) {
            result = spawnProcess({"spd-say", text});
        } else if (commandExists("espeak")) {
            result = spawnProcess({"espeak", text});
        }

        if (result != 0) {
            log("WARNING", "TTS playback failed for: " + text);
        }

        if (m_callback) m_callback(text, false);
    }
}

} // namespace VoiceAssistant

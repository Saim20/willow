#pragma once

#include "ModelPaths.hpp"
#include <string>
#include <queue>
#include <thread>
#include <mutex>
#include <condition_variable>
#include <atomic>
#include <functional>
#include <vector>

namespace VoiceAssistant {

struct TtsConfig {
    bool enabled{true};
    bool commandExecuted{true};
    bool modeChanged{true};
    bool searchExecuted{true};
    bool errors{true};
};

class TtsEngine {
public:
    using SpeakCallback = std::function<void(const std::string& text, bool started)>;

    TtsEngine();
    ~TtsEngine();

    bool initialize(const ModelPaths& paths);
    void shutdown();
    bool isLoaded() const { return m_loaded; }

    void setConfig(const TtsConfig& config) { m_config = config; }
    const TtsConfig& config() const { return m_config; }

    void speak(const std::string& text);
    void speakAsync(const std::string& text);
    void setCallback(SpeakCallback callback) { m_callback = callback; }

private:
    TtsConfig m_config;
    bool m_loaded{false};

    std::queue<std::string> m_queue;
    std::mutex m_mutex;
    std::condition_variable m_cv;
    std::thread m_worker;
    std::atomic<bool> m_stop{false};
    SpeakCallback m_callback;

    void workerLoop();
    void playAudio(const std::vector<float>& samples, int sampleRate);
    void log(const std::string& level, const std::string& message);
};

} // namespace VoiceAssistant

#pragma once

#include "ModelPaths.hpp"
#include "sherpa-onnx/c-api/cxx-api.h"
#include <string>
#include <vector>
#include <functional>
#include <memory>
#include <mutex>

namespace VoiceAssistant {

struct TranscriptionResult {
    std::string text;
    bool isFinal{false};
    bool isEndpoint{false};
};

class StreamingASREngine {
public:
    using TranscriptionCallback = std::function<void(const TranscriptionResult&)>;

    StreamingASREngine();
    ~StreamingASREngine();

    bool initialize(const ModelPaths& paths, int numThreads = 2, float endpointSilence = 0.3f);
    void shutdown();
    bool isLoaded() const { return m_loaded; }

    void setCallback(TranscriptionCallback callback);
    void setEndpointSilence(float seconds);
    void processAudio(const std::vector<float>& chunk);
    void resetStream();

    void setEnabled(bool enabled) { m_enabled = enabled; }
    bool isEnabled() const { return m_enabled; }

private:
    std::unique_ptr<sherpa_onnx::cxx::OnlineRecognizer> m_recognizer;
    std::unique_ptr<sherpa_onnx::cxx::OnlineStream> m_stream;
    TranscriptionCallback m_callback;
    std::mutex m_mutex;
    bool m_loaded{false};
    bool m_enabled{true};
    bool m_hasPaths{false};
    ModelPaths m_paths{""};
    int m_numThreads{2};
    float m_endpointSilence{0.3f};
    std::string m_lastPartial;

    void log(const std::string& level, const std::string& message);
};

} // namespace VoiceAssistant

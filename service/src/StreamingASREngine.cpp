#include "StreamingASREngine.hpp"
#include <algorithm>
#include <iostream>
#include "sherpa-onnx/c-api/cxx-api.h"

namespace VoiceAssistant {

StreamingASREngine::StreamingASREngine() = default;

StreamingASREngine::~StreamingASREngine() {
    shutdown();
}

void StreamingASREngine::log(const std::string& level, const std::string& message) {
    std::cout << "[StreamingASR] [" << level << "] " << message << std::endl;
}

bool StreamingASREngine::initialize(const ModelPaths& paths, int numThreads, float endpointSilence) {
    shutdown();

    m_paths = paths;
    m_numThreads = numThreads;
    m_endpointSilence = endpointSilence;
    m_hasPaths = true;

    const auto modelFiles = paths.findStreamingModel();
    if (!modelFiles) {
        log("ERROR", "Streaming ASR model not found in " + paths.basePath());
        return false;
    }

    sherpa_onnx::cxx::OnlineRecognizerConfig config;
    config.feat_config.sample_rate = 16000;
    config.feat_config.feature_dim = 80;
    config.model_config.tokens = modelFiles->tokens;
    config.model_config.transducer.encoder = modelFiles->encoder;
    config.model_config.transducer.decoder = modelFiles->decoder;
    config.model_config.transducer.joiner = modelFiles->joiner;
    config.model_config.num_threads = numThreads;
    config.model_config.provider = "cpu";
    config.decoding_method = "greedy_search";
    config.enable_endpoint = true;
    config.rule1_min_trailing_silence = endpointSilence;
    config.rule2_min_trailing_silence = std::max(0.5f, endpointSilence * 0.5f);
    config.rule3_min_utterance_length = 5.0f;

    try {
        m_recognizer = std::make_unique<sherpa_onnx::cxx::OnlineRecognizer>(
            sherpa_onnx::cxx::OnlineRecognizer::Create(config));
        m_stream = std::make_unique<sherpa_onnx::cxx::OnlineStream>(m_recognizer->CreateStream());
        m_loaded = true;
        log("INFO", "Streaming ASR initialized (endpoint silence: " + std::to_string(endpointSilence) + "s)");
        return true;
    } catch (const std::exception& e) {
        log("ERROR", std::string("Failed to initialize streaming ASR: ") + e.what());
        return false;
    }
}

void StreamingASREngine::shutdown() {
    m_stream.reset();
    m_recognizer.reset();
    m_loaded = false;
    m_lastPartial.clear();
}

void StreamingASREngine::setCallback(TranscriptionCallback callback) {
    std::lock_guard<std::mutex> lock(m_mutex);
    m_callback = std::move(callback);
}

void StreamingASREngine::setEndpointSilence(float seconds) {
    if (!m_hasPaths || seconds == m_endpointSilence) return;
    initialize(m_paths, m_numThreads, seconds);
}

void StreamingASREngine::resetStream() {
    std::lock_guard<std::mutex> lock(m_mutex);
    if (m_recognizer) {
        m_stream = std::make_unique<sherpa_onnx::cxx::OnlineStream>(m_recognizer->CreateStream());
    }
    m_lastPartial.clear();
}

void StreamingASREngine::processAudio(const std::vector<float>& chunk) {
    if (!m_loaded || !m_enabled || !m_recognizer || !m_stream || chunk.empty()) return;

    std::lock_guard<std::mutex> lock(m_mutex);

    m_stream->AcceptWaveform(16000, chunk.data(), static_cast<int32_t>(chunk.size()));

    while (m_recognizer->IsReady(m_stream.get())) {
        m_recognizer->Decode(m_stream.get());
    }

    auto result = m_recognizer->GetResult(m_stream.get());
    const std::string text = result.text;

    if (!text.empty() && text != m_lastPartial) {
        m_lastPartial = text;
        if (m_callback) {
            TranscriptionResult tr;
            tr.text = text;
            tr.isFinal = false;
            tr.isEndpoint = false;
            m_callback(tr);
        }
    }

    if (m_recognizer->IsEndpoint(m_stream.get())) {
        if (!text.empty() && m_callback) {
            TranscriptionResult tr;
            tr.text = text;
            tr.isFinal = true;
            tr.isEndpoint = true;
            m_callback(tr);
        }
        m_recognizer->Reset(m_stream.get());
        m_lastPartial.clear();
    }
}

} // namespace VoiceAssistant

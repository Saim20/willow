#include "SpeakerVerifier.hpp"
#include <fstream>
#include <iostream>
#include <thread>
#include <atomic>
#include <filesystem>
#include "sherpa-onnx/c-api/c-api.h"

namespace fs = std::filesystem;

namespace VoiceAssistant {

SpeakerVerifier::SpeakerVerifier() = default;

SpeakerVerifier::~SpeakerVerifier() {
    shutdown();
}

void SpeakerVerifier::log(const std::string& level, const std::string& message) {
    std::cout << "[SpeakerVerifier] [" << level << "] " << message << std::endl;
}

bool SpeakerVerifier::initialize(const ModelPaths& paths) {
    shutdown();

    const auto modelPath = paths.findSpeakerModel();
    if (!modelPath) {
        log("WARNING", "Speaker model not found - verification disabled");
        return false;
    }

    m_modelPath = *modelPath;
    m_profilePath = paths.speakerProfilePath();

    SherpaOnnxSpeakerEmbeddingExtractorConfig config{};
    config.model = m_modelPath.c_str();
    config.num_threads = 2;
    config.debug = 0;
    config.provider = "cpu";

    m_extractor = SherpaOnnxCreateSpeakerEmbeddingExtractor(&config);
    if (!m_extractor) {
        log("ERROR", "Failed to create speaker embedding extractor");
        return false;
    }

    const int32_t dim = SherpaOnnxSpeakerEmbeddingExtractorDim(m_extractor);
    m_manager = SherpaOnnxCreateSpeakerEmbeddingManager(dim);
    if (!m_manager) {
        log("ERROR", "Failed to create speaker embedding manager");
        SherpaOnnxDestroySpeakerEmbeddingExtractor(m_extractor);
        m_extractor = nullptr;
        return false;
    }

    m_loaded = true;
    loadProfile();
    log("INFO", "Speaker verifier initialized");
    return true;
}

void SpeakerVerifier::shutdown() {
    m_shuttingDown = true;
    {
        std::lock_guard<std::mutex> lock(m_verifyMutex);
        if (m_verifyThread.joinable()) {
            m_verifyThread.join();
        }
    }
    if (m_manager) {
        SherpaOnnxDestroySpeakerEmbeddingManager(m_manager);
        m_manager = nullptr;
    }
    if (m_extractor) {
        SherpaOnnxDestroySpeakerEmbeddingExtractor(m_extractor);
        m_extractor = nullptr;
    }
    m_loaded = false;
    m_enrolled = false;
}

std::vector<float> SpeakerVerifier::computeEmbedding(const std::vector<float>& audio) {
    if (!m_extractor || audio.empty()) return {};

    const SherpaOnnxOnlineStream* stream =
        SherpaOnnxSpeakerEmbeddingExtractorCreateStream(m_extractor);
    SherpaOnnxOnlineStreamAcceptWaveform(stream, 16000, audio.data(),
                                         static_cast<int32_t>(audio.size()));
    SherpaOnnxOnlineStreamInputFinished(stream);

    if (!SherpaOnnxSpeakerEmbeddingExtractorIsReady(m_extractor, stream)) {
        SherpaOnnxDestroyOnlineStream(stream);
        return {};
    }

    const float* emb = SherpaOnnxSpeakerEmbeddingExtractorComputeEmbedding(m_extractor, stream);
    const int32_t dim = SherpaOnnxSpeakerEmbeddingExtractorDim(m_extractor);
    std::vector<float> result(emb, emb + dim);
    SherpaOnnxSpeakerEmbeddingExtractorDestroyEmbedding(emb);
    SherpaOnnxDestroyOnlineStream(stream);
    return result;
}

bool SpeakerVerifier::verify(const std::vector<float>& audio) {
    if (!m_enabled || !m_loaded || !m_enrolled || !m_manager) {
        return !m_enabled;
    }

    const auto embedding = computeEmbedding(audio);
    if (embedding.empty()) {
        log("WARNING", "Failed to compute speaker embedding");
        return false;
    }

    const int32_t ok = SherpaOnnxSpeakerEmbeddingManagerVerify(
        m_manager, m_enrolledUser.c_str(), embedding.data(), m_threshold);
    log("INFO", std::string("Speaker verification: ") + (ok ? "passed" : "failed"));
    return ok != 0;
}

bool SpeakerVerifier::verifyAsync(const std::vector<float>& audio, VerificationCallback callback) {
    if (m_shuttingDown || !m_enabled) {
        if (callback) callback(!m_enabled, m_enabled ? "shutting down" : "disabled");
        return false;
    }

    std::lock_guard<std::mutex> lock(m_verifyMutex);
    if (m_shuttingDown) return false;
    if (m_verifyThread.joinable()) {
        m_verifyThread.join();
    }

    m_verifyThread = std::thread([this, audio, callback]() {
        const bool ok = verify(audio);
        if (!m_shuttingDown && callback) {
            callback(ok, ok ? "verified" : "unrecognized speaker");
        }
    });
    return true;
}

void SpeakerVerifier::startEnrollment(const std::string& userName) {
    std::lock_guard<std::mutex> lock(m_mutex);
    m_enrolledUser = userName;
    m_enrollmentSamples.clear();
    m_enrollmentBuffer.clear();
    m_enrollmentState = EnrollmentState::Recording;
    log("INFO", "Started speaker enrollment for: " + userName);
}

void SpeakerVerifier::addEnrollmentAudio(const std::vector<float>& audio) {
    std::lock_guard<std::mutex> lock(m_mutex);
    if (m_enrollmentState != EnrollmentState::Recording) return;

    m_enrollmentBuffer.insert(m_enrollmentBuffer.end(), audio.begin(), audio.end());
    constexpr size_t SAMPLE_LEN = 16000 * 2;
    while (m_enrollmentBuffer.size() >= SAMPLE_LEN) {
        m_enrollmentSamples.emplace_back(
            m_enrollmentBuffer.begin(),
            m_enrollmentBuffer.begin() + static_cast<std::ptrdiff_t>(SAMPLE_LEN));
        m_enrollmentBuffer.erase(
            m_enrollmentBuffer.begin(),
            m_enrollmentBuffer.begin() + static_cast<std::ptrdiff_t>(SAMPLE_LEN));
        log("INFO", "Enrollment sample " + std::to_string(m_enrollmentSamples.size()) + " recorded");
    }
}

bool SpeakerVerifier::finishEnrollment() {
    std::lock_guard<std::mutex> lock(m_mutex);
    if (!m_loaded || !m_manager || m_enrollmentSamples.size() < 2) {
        m_enrollmentState = EnrollmentState::Failed;
        return false;
    }

    std::vector<std::vector<float>> embeddings;
    for (const auto& sample : m_enrollmentSamples) {
        auto emb = computeEmbedding(sample);
        if (!emb.empty()) embeddings.push_back(std::move(emb));
    }

    if (embeddings.size() < 2) {
        m_enrollmentState = EnrollmentState::Failed;
        return false;
    }

    std::vector<float> flat;
    for (const auto& emb : embeddings) {
        flat.insert(flat.end(), emb.begin(), emb.end());
    }

    SherpaOnnxSpeakerEmbeddingManagerRemove(m_manager, m_enrolledUser.c_str());
    const int32_t ok = SherpaOnnxSpeakerEmbeddingManagerAddListFlattened(
        m_manager, m_enrolledUser.c_str(), flat.data(),
        static_cast<int32_t>(embeddings.size()));

    if (ok) {
        m_enrolled = true;
        m_enrollmentState = EnrollmentState::Complete;
        // Persist embeddings
        fs::create_directories(fs::path(m_profilePath).parent_path());
        std::ofstream file(m_profilePath, std::ios::binary);
        if (file) {
            const int32_t dim = SherpaOnnxSpeakerEmbeddingExtractorDim(m_extractor);
            const int32_t count = static_cast<int32_t>(embeddings.size());
            file.write(reinterpret_cast<const char*>(&dim), sizeof(dim));
            file.write(reinterpret_cast<const char*>(&count), sizeof(count));
            for (const auto& emb : embeddings) {
                file.write(reinterpret_cast<const char*>(emb.data()), dim * sizeof(float));
            }
        }
        log("INFO", "Speaker enrollment complete");
    } else {
        m_enrollmentState = EnrollmentState::Failed;
    }
    m_enrollmentSamples.clear();
    m_enrollmentBuffer.clear();
    return ok != 0;
}

void SpeakerVerifier::cancelEnrollment() {
    std::lock_guard<std::mutex> lock(m_mutex);
    m_enrollmentSamples.clear();
    m_enrollmentBuffer.clear();
    m_enrollmentState = EnrollmentState::Idle;
}

void SpeakerVerifier::removeProfile() {
    std::lock_guard<std::mutex> lock(m_mutex);
    if (m_manager) {
        SherpaOnnxSpeakerEmbeddingManagerRemove(m_manager, m_enrolledUser.c_str());
    }
    m_enrolled = false;
    if (fs::exists(m_profilePath)) fs::remove(m_profilePath);
}

bool SpeakerVerifier::loadProfile() {
    if (!m_loaded || !m_manager || !fs::exists(m_profilePath)) return false;

    std::ifstream file(m_profilePath, std::ios::binary);
    if (!file) return false;

    int32_t dim = 0, count = 0;
    file.read(reinterpret_cast<char*>(&dim), sizeof(dim));
    file.read(reinterpret_cast<char*>(&count), sizeof(count));
    if (dim != SherpaOnnxSpeakerEmbeddingExtractorDim(m_extractor) || count <= 0) return false;

    std::vector<std::vector<float>> embeddings;
    for (int32_t i = 0; i < count; ++i) {
        std::vector<float> emb(dim);
        file.read(reinterpret_cast<char*>(emb.data()), dim * sizeof(float));
        embeddings.push_back(std::move(emb));
    }

    std::vector<float> flat;
    for (const auto& emb : embeddings) {
        flat.insert(flat.end(), emb.begin(), emb.end());
    }

    SherpaOnnxSpeakerEmbeddingManagerRemove(m_manager, m_enrolledUser.c_str());
    const int32_t ok = SherpaOnnxSpeakerEmbeddingManagerAddListFlattened(
        m_manager, m_enrolledUser.c_str(), flat.data(), count);
    m_enrolled = ok != 0;
    if (m_enrolled) log("INFO", "Speaker profile loaded");
    return m_enrolled;
}

} // namespace VoiceAssistant

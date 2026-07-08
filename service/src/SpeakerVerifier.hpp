#pragma once

#include "ModelPaths.hpp"
#include <string>
#include <vector>
#include <functional>
#include <mutex>
#include <thread>
#include <atomic>

struct SherpaOnnxSpeakerEmbeddingExtractor;
struct SherpaOnnxSpeakerEmbeddingManager;
struct SherpaOnnxOnlineStream;

namespace VoiceAssistant {

enum class EnrollmentState {
    Idle,
    Recording,
    Complete,
    Failed
};

class SpeakerVerifier {
public:
    using VerificationCallback = std::function<void(bool verified, const std::string& reason)>;

    SpeakerVerifier();
    ~SpeakerVerifier();

    bool initialize(const ModelPaths& paths);
    void shutdown();
    bool isLoaded() const { return m_loaded; }
    bool isEnrolled() const { return m_enrolled; }

    void setEnabled(bool enabled) { m_enabled = enabled; }
    bool isEnabled() const { return m_enabled; }
    void setThreshold(float threshold) { m_threshold = threshold; }
    void setEnrolledUser(const std::string& name) { m_enrolledUser = name; }

    bool verify(const std::vector<float>& audio);
    bool verifyAsync(const std::vector<float>& audio, VerificationCallback callback);

    void startEnrollment(const std::string& userName);
    void addEnrollmentAudio(const std::vector<float>& audio);
    bool finishEnrollment();
    void cancelEnrollment();
    void removeProfile();
    EnrollmentState enrollmentState() const { return m_enrollmentState; }
    int enrollmentProgress() const { return static_cast<int>(m_enrollmentSamples.size()); }

    bool loadProfile();

private:
    const SherpaOnnxSpeakerEmbeddingExtractor* m_extractor{nullptr};
    const SherpaOnnxSpeakerEmbeddingManager* m_manager{nullptr};
    std::string m_modelPath;
    std::string m_profilePath;
    std::string m_enrolledUser{"owner"};
    float m_threshold{0.65f};
    bool m_loaded{false};
    bool m_enrolled{false};
    bool m_enabled{true};

    EnrollmentState m_enrollmentState{EnrollmentState::Idle};
    std::vector<std::vector<float>> m_enrollmentSamples;
    std::vector<float> m_enrollmentBuffer;
    std::mutex m_mutex;
    std::mutex m_verifyMutex;
    std::thread m_verifyThread;
    std::atomic<bool> m_shuttingDown{false};

    std::vector<float> computeEmbedding(const std::vector<float>& audio);
    void log(const std::string& level, const std::string& message);
};

} // namespace VoiceAssistant

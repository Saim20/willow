#pragma once

#include <vector>
#include <deque>
#include <mutex>
#include <functional>
#include <cstddef>

namespace VoiceAssistant {

/**
 * AudioRouter - fan-out mic chunks to engines and maintain a rolling buffer
 * for speaker verification.
 */
class AudioRouter {
public:
    using AudioCallback = std::function<void(const std::vector<float>&)>;

    static constexpr int SAMPLE_RATE = 16000;
    static constexpr size_t ROLLING_BUFFER_SECONDS = 3;

    void addConsumer(AudioCallback callback);
    void processChunk(const std::vector<float>& chunk);

    std::vector<float> getRecentAudio(float seconds) const;
    void clearRollingBuffer();

private:
    std::vector<AudioCallback> m_consumers;
    std::mutex m_consumerMutex;

    std::deque<float> m_rollingBuffer;
    mutable std::mutex m_bufferMutex;
    static constexpr size_t MAX_ROLLING_SAMPLES = SAMPLE_RATE * ROLLING_BUFFER_SECONDS;
};

} // namespace VoiceAssistant

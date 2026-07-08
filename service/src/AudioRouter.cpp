#include "AudioRouter.hpp"
#include <algorithm>

namespace VoiceAssistant {

void AudioRouter::addConsumer(AudioCallback callback) {
    std::lock_guard<std::mutex> lock(m_consumerMutex);
    m_consumers.push_back(std::move(callback));
}

void AudioRouter::processChunk(const std::vector<float>& chunk) {
    {
        std::lock_guard<std::mutex> lock(m_bufferMutex);
        for (float sample : chunk) {
            m_rollingBuffer.push_back(sample);
        }
        while (m_rollingBuffer.size() > MAX_ROLLING_SAMPLES) {
            m_rollingBuffer.pop_front();
        }
    }

    std::lock_guard<std::mutex> lock(m_consumerMutex);
    for (const auto& consumer : m_consumers) {
        if (consumer) {
            consumer(chunk);
        }
    }
}

std::vector<float> AudioRouter::getRecentAudio(float seconds) const {
    std::lock_guard<std::mutex> lock(m_bufferMutex);
    const size_t sampleCount = static_cast<size_t>(seconds * SAMPLE_RATE);
    const size_t start = m_rollingBuffer.size() > sampleCount
        ? m_rollingBuffer.size() - sampleCount
        : 0;

    return std::vector<float>(m_rollingBuffer.begin() + static_cast<std::ptrdiff_t>(start),
                              m_rollingBuffer.end());
}

void AudioRouter::clearRollingBuffer() {
    std::lock_guard<std::mutex> lock(m_bufferMutex);
    m_rollingBuffer.clear();
}

} // namespace VoiceAssistant

#pragma once

#include <whisper.h>
#include <vector>
#include <string>
#include <functional>
#include <mutex>
#include <atomic>
#include <thread>
#include <queue>
#include <condition_variable>

namespace VoiceAssistant {

struct SpeechSegment {
    std::vector<float> samples;
    float peakEnergy = 0.0f;
};

/**
 * SpeechSegmenter - VAD-based segmentation with async Whisper inference
 */
class SpeechSegmenter {
public:
    using TranscriptionCallback = std::function<void(const std::string&)>;
    
    SpeechSegmenter();
    ~SpeechSegmenter();
    
    bool initialize(const std::string& modelPath, const std::string& modelFile, bool useGPU);
    void shutdown();
    
    void processAudioChunk(const std::vector<float>& chunk);
    void setTranscriptionCallback(TranscriptionCallback callback);
    
    void setVADThreshold(float threshold);
    void setSilenceDuration(float seconds);
    void setMinSpeechDuration(float seconds);
    void setThreadCount(int threads);
    void setEnergyFilterEnabled(bool enabled);
    void setMinPeakEnergy(float energy);
    
    bool isWhisperLoaded() const { return m_whisperCtx != nullptr; }
    bool isSpeaking() const { return m_isSpeaking; }

private:
    whisper_context* m_whisperCtx;
    whisper_full_params m_whisperParams;
    std::string m_modelPath;
    
    mutable std::mutex m_configMutex;
    float m_vadThreshold;
    float m_silenceDuration;
    float m_minSpeechDuration;
    bool m_energyFilterEnabled;
    float m_minPeakEnergy;
    
    std::atomic<bool> m_isSpeaking;
    std::vector<float> m_speechBuffer;
    std::vector<float> m_frameBuffer;
    float m_segmentPeakEnergy;
    int m_silenceFrames;
    int m_speechFrames;
    
    static constexpr int SAMPLE_RATE = 16000;
    static constexpr int FRAMES_PER_SECOND = 50;
    static constexpr int FRAME_SIZE = SAMPLE_RATE / FRAMES_PER_SECOND;
    static constexpr size_t MAX_QUEUE_DEPTH = 3;
    
    TranscriptionCallback m_callback;
    std::mutex m_callbackMutex;
    
    std::thread m_inferenceThread;
    std::atomic<bool> m_stopInference{false};
    std::queue<SpeechSegment> m_segmentQueue;
    std::mutex m_queueMutex;
    std::condition_variable m_queueCv;
    
    void startInferenceThread();
    void stopInferenceThread();
    void inferenceLoop();
    void enqueueSegment(SpeechSegment segment);
    
    bool detectVoiceActivity(const std::vector<float>& frame);
    float calculateEnergy(const std::vector<float>& frame);
    
    std::string transcribe(const std::vector<float>& samples);
    std::string cleanTranscription(const std::string& text);
    void warmupModel();
    
    void log(const std::string& level, const std::string& message);
    std::mutex m_logMutex;
};

} // namespace VoiceAssistant

#include "SpeechSegmenter.hpp"
#include <cmath>
#include <algorithm>
#include <regex>
#include <iostream>
#include <fstream>
#include <ctime>
#include <iomanip>

namespace VoiceAssistant {

SpeechSegmenter::SpeechSegmenter()
    : m_whisperCtx(nullptr)
    , m_vadThreshold(0.0003f)
    , m_silenceDuration(0.8f)
    , m_minSpeechDuration(0.25f)
    , m_energyFilterEnabled(false)
    , m_minPeakEnergy(0.001f)
    , m_isSpeaking(false)
    , m_segmentPeakEnergy(0.0f)
    , m_silenceFrames(0)
    , m_speechFrames(0)
{
    m_frameBuffer.resize(FRAME_SIZE);
}

SpeechSegmenter::~SpeechSegmenter() {
    shutdown();
}

void SpeechSegmenter::setVADThreshold(float threshold) {
    std::lock_guard<std::mutex> lock(m_configMutex);
    m_vadThreshold = threshold;
}

void SpeechSegmenter::setSilenceDuration(float seconds) {
    std::lock_guard<std::mutex> lock(m_configMutex);
    m_silenceDuration = seconds;
}

void SpeechSegmenter::setMinSpeechDuration(float seconds) {
    std::lock_guard<std::mutex> lock(m_configMutex);
    m_minSpeechDuration = seconds;
}

void SpeechSegmenter::setThreadCount(int threads) {
    std::lock_guard<std::mutex> lock(m_configMutex);
    if (threads > 0) {
        m_whisperParams.n_threads = threads;
    }
}

void SpeechSegmenter::setEnergyFilterEnabled(bool enabled) {
    m_energyFilterEnabled = enabled;
}

void SpeechSegmenter::setMinPeakEnergy(float energy) {
    m_minPeakEnergy = energy;
}

bool SpeechSegmenter::initialize(const std::string& modelPath, const std::string& modelFile, bool useGPU) {
    m_modelPath = modelPath + "/" + modelFile;
    
    whisper_context_params cparams = whisper_context_default_params();
    cparams.use_gpu = useGPU;
    cparams.gpu_device = 0;
    
    log("INFO", "Initializing Whisper from: " + m_modelPath + " (GPU: " + 
        std::string(useGPU ? "enabled" : "disabled") + ")");
    
    m_whisperCtx = whisper_init_from_file_with_params(m_modelPath.c_str(), cparams);
    
    if (!m_whisperCtx) {
        log("ERROR", "Failed to load Whisper model from: " + m_modelPath);
        return false;
    }
    
    m_whisperParams = whisper_full_default_params(WHISPER_SAMPLING_GREEDY);
    m_whisperParams.print_progress = false;
    m_whisperParams.print_timestamps = false;
    m_whisperParams.print_special = false;
    m_whisperParams.language = "en";
    m_whisperParams.n_threads = 4;
    m_whisperParams.translate = false;
    m_whisperParams.no_context = true;
    m_whisperParams.single_segment = false;
    
    startInferenceThread();
    warmupModel();
    
    log("INFO", "Whisper initialized successfully");
    return true;
}

void SpeechSegmenter::shutdown() {
    stopInferenceThread();
    
    if (m_whisperCtx) {
        whisper_free(m_whisperCtx);
        m_whisperCtx = nullptr;
        log("INFO", "Whisper context freed");
    }
}

void SpeechSegmenter::startInferenceThread() {
    m_stopInference = false;
    m_inferenceThread = std::thread(&SpeechSegmenter::inferenceLoop, this);
}

void SpeechSegmenter::stopInferenceThread() {
    m_stopInference = true;
    m_queueCv.notify_all();
    
    if (m_inferenceThread.joinable()) {
        m_inferenceThread.join();
    }
    
    std::lock_guard<std::mutex> lock(m_queueMutex);
    while (!m_segmentQueue.empty()) {
        m_segmentQueue.pop();
    }
}

void SpeechSegmenter::enqueueSegment(SpeechSegment segment) {
    if (m_energyFilterEnabled && segment.peakEnergy < m_minPeakEnergy) {
        log("INFO", "Skipping low-energy segment (peak: " + std::to_string(segment.peakEnergy) + ")");
        return;
    }

    std::lock_guard<std::mutex> lock(m_queueMutex);
    while (m_segmentQueue.size() >= MAX_QUEUE_DEPTH) {
        m_segmentQueue.pop();
        log("WARNING", "Inference queue full, dropping oldest segment");
    }
    m_segmentQueue.push(std::move(segment));
    m_queueCv.notify_one();
}

void SpeechSegmenter::inferenceLoop() {
    while (!m_stopInference) {
        SpeechSegment segment;
        
        {
            std::unique_lock<std::mutex> lock(m_queueMutex);
            m_queueCv.wait(lock, [this]() {
                return m_stopInference || !m_segmentQueue.empty();
            });
            
            if (m_stopInference) {
                break;
            }
            
            segment = std::move(m_segmentQueue.front());
            m_segmentQueue.pop();
        }
        
        std::string transcription = transcribe(segment.samples);
        if (!transcription.empty()) {
            log("INFO", "Transcription: " + transcription);
            
            std::lock_guard<std::mutex> lock(m_callbackMutex);
            if (m_callback) {
                m_callback(transcription);
            }
        }
    }
}

void SpeechSegmenter::processAudioChunk(const std::vector<float>& chunk) {
    if (!m_whisperCtx) return;
    
    float vadThreshold;
    float silenceDuration;
    float minSpeechDuration;
    {
        std::lock_guard<std::mutex> lock(m_configMutex);
        vadThreshold = m_vadThreshold;
        silenceDuration = m_silenceDuration;
        minSpeechDuration = m_minSpeechDuration;
    }
    
    for (size_t i = 0; i + FRAME_SIZE <= chunk.size(); i += FRAME_SIZE) {
        std::copy(chunk.begin() + i, chunk.begin() + i + FRAME_SIZE, m_frameBuffer.begin());
        
        float energy = calculateEnergy(m_frameBuffer);
        bool voiceDetected = energy > vadThreshold;
        
        if (voiceDetected) {
            if (!m_isSpeaking) {
                log("INFO", "Speech started");
                m_isSpeaking = true;
                m_speechBuffer.clear();
                m_segmentPeakEnergy = 0.0f;
            }
            
            m_segmentPeakEnergy = std::max(m_segmentPeakEnergy, energy);
            m_speechBuffer.insert(m_speechBuffer.end(), m_frameBuffer.begin(), m_frameBuffer.end());
            m_silenceFrames = 0;
            m_speechFrames++;
            
        } else if (m_isSpeaking) {
            m_speechBuffer.insert(m_speechBuffer.end(), m_frameBuffer.begin(), m_frameBuffer.end());
            m_silenceFrames++;
            
            int silenceThresholdFrames = static_cast<int>(silenceDuration * FRAMES_PER_SECOND);
            if (m_silenceFrames >= silenceThresholdFrames) {
                float speechDuration = static_cast<float>(m_speechFrames) / FRAMES_PER_SECOND;
                
                log("INFO", "Speech ended (duration: " + std::to_string(speechDuration) + "s)");
                
                if (speechDuration >= minSpeechDuration) {
                    SpeechSegment segment;
                    segment.samples = std::move(m_speechBuffer);
                    segment.peakEnergy = m_segmentPeakEnergy;
                    enqueueSegment(std::move(segment));
                } else {
                    log("INFO", "Speech too short, ignoring (duration: " + 
                        std::to_string(speechDuration) + "s)");
                }
                
                m_isSpeaking = false;
                m_speechBuffer.clear();
                m_silenceFrames = 0;
                m_speechFrames = 0;
                m_segmentPeakEnergy = 0.0f;
            }
        }
    }
}

void SpeechSegmenter::setTranscriptionCallback(TranscriptionCallback callback) {
    std::lock_guard<std::mutex> lock(m_callbackMutex);
    m_callback = callback;
}

bool SpeechSegmenter::detectVoiceActivity(const std::vector<float>& frame) {
    float energy = calculateEnergy(frame);
    std::lock_guard<std::mutex> lock(m_configMutex);
    return energy > m_vadThreshold;
}

float SpeechSegmenter::calculateEnergy(const std::vector<float>& frame) {
    if (frame.empty()) return 0.0f;
    
    float sum = 0.0f;
    for (float sample : frame) {
        sum += sample * sample;
    }
    
    return sum / frame.size();
}

std::string SpeechSegmenter::transcribe(const std::vector<float>& samples) {
    if (!m_whisperCtx || samples.empty()) {
        return "";
    }
    
    if (whisper_full(m_whisperCtx, m_whisperParams, samples.data(), samples.size()) != 0) {
        log("ERROR", "Whisper transcription failed");
        return "";
    }
    
    const int n_segments = whisper_full_n_segments(m_whisperCtx);
    std::string result;
    
    for (int i = 0; i < n_segments; ++i) {
        const char* text = whisper_full_get_segment_text(m_whisperCtx, i);
        if (text) {
            result += text;
        }
    }
    
    return cleanTranscription(result);
}

void SpeechSegmenter::warmupModel() {
    if (!m_whisperCtx) return;
    
    std::vector<float> silence(FRAME_SIZE, 0.0f);
    whisper_full(m_whisperCtx, m_whisperParams, silence.data(), silence.size());
    log("INFO", "Whisper model warmed up");
}

std::string SpeechSegmenter::cleanTranscription(const std::string& text) {
    std::string result = text;
    
    std::regex bracketPattern(R"(\[[^\]]*\]|\{[^\}]*\}|\([^\)]*\))");
    result = std::regex_replace(result, bracketPattern, "");
    
    std::regex punctPattern(R"([.,!?;:])");
    result = std::regex_replace(result, punctPattern, "");
    
    std::regex multiSpacePattern(R"(\s+)");
    result = std::regex_replace(result, multiSpacePattern, " ");
    
    result.erase(0, result.find_first_not_of(" \t\n\r"));
    result.erase(result.find_last_not_of(" \t\n\r") + 1);
    
    std::transform(result.begin(), result.end(), result.begin(), ::tolower);
    
    return result;
}

void SpeechSegmenter::log(const std::string& level, const std::string& message) {
    std::lock_guard<std::mutex> lock(m_logMutex);
    
    auto now = std::time(nullptr);
    auto tm = *std::localtime(&now);
    
    std::ofstream logFile("/tmp/willow.log", std::ios::app);
    if (logFile.is_open()) {
        logFile << std::put_time(&tm, "%Y-%m-%d %H:%M:%S") 
                << " [SpeechSegmenter] [" << level << "] " << message << std::endl;
    }
    
    std::cout << "[SpeechSegmenter] [" << level << "] " << message << std::endl;
}

} // namespace VoiceAssistant

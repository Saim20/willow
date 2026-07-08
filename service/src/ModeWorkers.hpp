#pragma once

#include "CommandExecutor.hpp"
#include "SpeechPipeline.hpp"
#include "CommandIntentResolver.hpp"
#include "Types.hpp"
#include <string>
#include <memory>
#include <atomic>
#include <functional>
#include <mutex>

namespace VoiceAssistant {

class ModeWorker {
public:
    using ModeChangeCallback = std::function<void(const std::string&)>;
    
    virtual ~ModeWorker() = default;
    
    virtual void start() = 0;
    virtual void stop() = 0;
    virtual bool isRunning() const = 0;
    
    virtual void processTranscription(const TranscriptionResult& result) = 0;
    virtual void processKeyword(const std::string& keyword) = 0;
    
    void setModeChangeCallback(ModeChangeCallback callback) {
        m_modeChangeCallback = callback;
    }
    
    virtual std::string getBuffer() const = 0;

protected:
    ModeChangeCallback m_modeChangeCallback;
    std::shared_ptr<CommandExecutor> m_executor;
    std::shared_ptr<SpeechPipeline> m_pipeline;
    
    void requestModeChange(const std::string& newMode) {
        if (m_modeChangeCallback) {
            m_modeChangeCallback(newMode);
        }
    }
};

class NormalModeWorker : public ModeWorker {
public:
    NormalModeWorker(std::shared_ptr<CommandExecutor> executor,
                     std::shared_ptr<SpeechPipeline> pipeline);
    
    void start() override;
    void stop() override;
    bool isRunning() const override { return m_isRunning; }
    
    void processTranscription(const TranscriptionResult& result) override;
    void processKeyword(const std::string& keyword) override;
    
    void setHotword(const std::string& hotword) { m_hotword = hotword; }
    std::string getBuffer() const override { return ""; }

private:
    std::atomic<bool> m_isRunning{false};
    std::string m_hotword;
};

class CommandModeWorker : public ModeWorker {
public:
    using CommandExecutedCallback = std::function<void(const std::string&, const std::string&, double)>;

    CommandModeWorker(std::shared_ptr<CommandExecutor> executor,
                      std::shared_ptr<SpeechPipeline> pipeline);
    
    void start() override;
    void stop() override;
    bool isRunning() const override { return m_isRunning; }
    
    void processTranscription(const TranscriptionResult& result) override;
    void processKeyword(const std::string& keyword) override;
    
    void setCommands(const std::vector<Command>& commands);
    void setThreshold(double threshold);
    void setCommandExecutedCallback(CommandExecutedCallback callback) {
        m_commandExecutedCallback = callback;
    }
    
    std::string getBuffer() const override;

private:
    std::atomic<bool> m_isRunning{false};
    CommandExecutedCallback m_commandExecutedCallback;
    
    std::string m_buffer;
    mutable std::mutex m_bufferMutex;

    void dispatchResult(const CommandDispatchResult& result);
    void executeDispatch(const CommandDispatchResult& result);
};

class TypingModeWorker : public ModeWorker {
public:
    TypingModeWorker(std::shared_ptr<CommandExecutor> executor,
                     std::shared_ptr<SpeechPipeline> pipeline);
    
    void start() override;
    void stop() override;
    bool isRunning() const override { return m_isRunning; }
    
    void processTranscription(const TranscriptionResult& result) override;
    void processKeyword(const std::string& keyword) override;
    
    void setExitPhrases(const std::vector<std::string>& phrases) {
        m_exitPhrases = phrases;
    }
    
    std::string getBuffer() const override;

private:
    std::atomic<bool> m_isRunning{false};
    std::vector<std::string> m_exitPhrases;
    
    std::string m_buffer;
    mutable std::mutex m_bufferMutex;
    
    bool checkExitPhrases(const std::string& text) const;
};

} // namespace VoiceAssistant

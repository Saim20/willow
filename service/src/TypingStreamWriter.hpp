#pragma once

#include "CommandExecutor.hpp"
#include <string>
#include <memory>

namespace VoiceAssistant {

class TypingStreamWriter {
public:
    explicit TypingStreamWriter(std::shared_ptr<CommandExecutor> executor);

    void setMaxBackspace(int max) { m_maxBackspace = max; }
    void setCheckRecentChars(int chars) { m_checkRecentChars = chars; }

    void processPartial(const std::string& partial);
    void processFinal(const std::string& finalText);
    void reset();

    std::string committedText() const { return m_committed; }
    std::string currentPartial() const { return m_lastPartial; }

private:
    std::shared_ptr<CommandExecutor> m_executor;
    std::string m_committed;
    std::string m_lastPartial;
    int m_maxBackspace{20};
    int m_checkRecentChars{100};

    void typeDelta(const std::string& oldText, const std::string& newText);
    void backspace(int count);
};

} // namespace VoiceAssistant

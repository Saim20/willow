#include "TypingStreamWriter.hpp"
#include <algorithm>

namespace VoiceAssistant {

TypingStreamWriter::TypingStreamWriter(std::shared_ptr<CommandExecutor> executor)
    : m_executor(std::move(executor)) {}

void TypingStreamWriter::backspace(int count) {
    const int capped = std::min(count, m_maxBackspace);
    for (int i = 0; i < capped; ++i) {
        m_executor->pressKey("14:1 14:0");
    }
}

void TypingStreamWriter::typeDelta(const std::string& oldText, const std::string& newText) {
    if (newText == oldText) return;

    // Find common prefix
    size_t commonLen = 0;
    const size_t minLen = std::min(oldText.size(), newText.size());
    while (commonLen < minLen && oldText[commonLen] == newText[commonLen]) {
        ++commonLen;
    }

    const int toDelete = static_cast<int>(oldText.size() - commonLen);
    if (toDelete > 0) {
        backspace(std::min(toDelete, m_checkRecentChars));
    }

    if (commonLen < newText.size()) {
        m_executor->typeText(newText.substr(commonLen));
    }
}

void TypingStreamWriter::processPartial(const std::string& partial) {
    if (partial.empty()) return;
    typeDelta(m_lastPartial, partial);
    m_lastPartial = partial;
}

void TypingStreamWriter::processFinal(const std::string& finalText) {
    if (!finalText.empty()) {
        typeDelta(m_lastPartial, finalText);
        m_executor->typeText(" ");
        m_committed += finalText + " ";
    }
    m_lastPartial.clear();
}

void TypingStreamWriter::reset() {
    m_lastPartial.clear();
}

} // namespace VoiceAssistant

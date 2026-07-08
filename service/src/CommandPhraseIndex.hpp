#pragma once

#include "CommandExecutor.hpp"
#include <string>
#include <vector>
#include <map>
#include <optional>

namespace VoiceAssistant {

struct PhraseEntry {
    std::string phrase;
    std::string commandName;
    std::string commandAction;
    bool hasPrefixExtension{false};
};

struct PhraseLookupResult {
    bool exactMatch{false};
    bool blockedByPrefix{false};
    std::vector<PhraseEntry> matches;
};

class CommandPhraseIndex {
public:
    void build(const std::vector<Command>& commands);
    PhraseLookupResult lookup(const std::string& text) const;
    bool hasPrefixExtension(const std::string& phrase) const;
    const std::vector<PhraseEntry>& allEntries() const { return m_entries; }

private:
    std::vector<PhraseEntry> m_entries;
    std::map<std::string, std::vector<size_t>> m_phraseIndex;

    static std::string normalize(const std::string& text);
    void computePrefixConflicts();
};

} // namespace VoiceAssistant

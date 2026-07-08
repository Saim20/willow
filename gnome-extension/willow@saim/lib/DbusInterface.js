/**
 * DbusInterface.js - Canonical D-Bus interface definition for Willow
 * Keep in sync with dbus/com.github.saim.Willow.xml
 */

import Gio from 'gi://Gio';

export const VoiceAssistantIface = `
<node>
  <interface name="com.github.saim.Willow">
    <method name="SetMode">
      <arg direction="in" name="mode" type="s"/>
    </method>
    <method name="GetMode">
      <arg direction="out" name="mode" type="s"/>
    </method>
    <method name="GetStatus">
      <arg direction="out" name="status" type="a{sv}"/>
    </method>
    <method name="GetConfig">
      <arg direction="out" name="config" type="s"/>
    </method>
    <method name="UpdateConfig">
      <arg direction="in" name="config" type="s"/>
    </method>
    <method name="SetConfigValue">
      <arg direction="in" name="key" type="s"/>
      <arg direction="in" name="value" type="v"/>
    </method>
    <method name="GetCommands">
      <arg direction="out" name="commands" type="s"/>
    </method>
    <method name="AddCommand">
      <arg direction="in" name="name" type="s"/>
      <arg direction="in" name="command" type="s"/>
      <arg direction="in" name="phrases" type="as"/>
    </method>
    <method name="RemoveCommand">
      <arg direction="in" name="name" type="s"/>
    </method>
    <method name="Start"/>
    <method name="Stop"/>
    <method name="Restart"/>
    <method name="GetBuffer">
      <arg direction="out" name="buffer" type="s"/>
    </method>
    <signal name="ModeChanged">
      <arg name="new_mode" type="s"/>
      <arg name="old_mode" type="s"/>
    </signal>
    <signal name="BufferChanged">
      <arg name="buffer" type="s"/>
    </signal>
    <signal name="PartialBufferChanged">
      <arg name="partial" type="s"/>
      <arg name="is_final" type="b"/>
    </signal>
    <signal name="CommandPending">
      <arg name="phrase" type="s"/>
      <arg name="blocked_by_prefix" type="b"/>
    </signal>
    <signal name="SpeakerVerificationFailed">
      <arg name="reason" type="s"/>
    </signal>
    <signal name="TtsStarted">
      <arg name="text" type="s"/>
    </signal>
    <signal name="TtsFinished"/>
    <method name="StartSpeakerEnrollment"/>
    <method name="CancelSpeakerEnrollment"/>
    <method name="GetSpeakerEnrollmentStatus">
      <arg direction="out" name="status" type="a{sv}"/>
    </method>
    <method name="RemoveSpeakerProfile"/>
    <signal name="CommandExecuted">
      <arg name="command" type="s"/>
      <arg name="phrase" type="s"/>
      <arg name="confidence" type="d"/>
    </signal>
    <signal name="StatusChanged">
      <arg name="status" type="a{sv}"/>
    </signal>
    <signal name="Error">
      <arg name="message" type="s"/>
      <arg name="details" type="s"/>
    </signal>
    <signal name="Notification">
      <arg name="title" type="s"/>
      <arg name="message" type="s"/>
      <arg name="urgency" type="s"/>
    </signal>
    <signal name="ConfigChanged">
      <arg name="config" type="s"/>
    </signal>
    <property name="IsRunning" type="b" access="read"/>
    <property name="CurrentMode" type="s" access="read"/>
    <property name="CurrentBuffer" type="s" access="read"/>
    <property name="Version" type="s" access="read"/>
  </interface>
</node>`;

export const DBUS_NAME = 'com.github.saim.Willow';
export const DBUS_PATH = '/com/github/saim/VoiceAssistant';

export function createVoiceAssistantProxy(callback) {
    const VoiceAssistantProxy = Gio.DBusProxy.makeProxyWrapper(VoiceAssistantIface);
    return new VoiceAssistantProxy(
        Gio.DBus.session,
        DBUS_NAME,
        DBUS_PATH,
        callback
    );
}

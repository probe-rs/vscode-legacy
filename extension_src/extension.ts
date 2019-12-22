'use strict';

import * as vscode from 'vscode';
import { DebugAdapterTracker, DebugAdapterTrackerFactory,  } from 'vscode';
import { exec } from 'child_process';

export function activate(context: vscode.ExtensionContext) {
    const tracker_factory = new ProbeRsDebugAdapterTrackerFactory();

    const descriptor_factory = new ProbeRsDebugAdapterDescriptorFactory();

    context.subscriptions.push(vscode.debug.registerDebugAdapterTrackerFactory('probe_rs', tracker_factory));
    context.subscriptions.push(vscode.debug.registerDebugAdapterDescriptorFactory('probe_rs', descriptor_factory));
}

class ProbeRsDebugAdapterTrackerFactory implements DebugAdapterTrackerFactory {
    createDebugAdapterTracker(session: vscode.DebugSession): vscode.ProviderResult<vscode.DebugAdapterTracker> {
        console.log("Creating new debug adapter tracker");

        const tracker = new ProbeRsDebugAdapterTracker();

        return tracker;
    }
}

class ProbeRsDebugAdapterTracker implements DebugAdapterTracker {
    onWillReceiveMessage(message: any) {
        console.log("Sending message to debug adapter:\n", message);
    }

    onDidSendMessage(message: any) {
        console.log("Received message from debug adapter:\n", message);
    }

    onError(error: Error) {
        console.log("Error in communication with debug adapter:\n", error);
    }

    onExit(code: number, signal: string) {
        if (code) {
            console.log("Debug Adapter exited with exit code", code);
        } else {
            console.log("Debug Adapter exited with signal", signal);
        }
    }
}



class ProbeRsDebugAdapterDescriptorFactory implements vscode.DebugAdapterDescriptorFactory {


    createDebugAdapterDescriptor(session: vscode.DebugSession, executable: vscode.DebugAdapterExecutable | undefined): vscode.ProviderResult<vscode.DebugAdapterDescriptor> {
        console.log("Session: ", session);
        console.log("Configuration: ", session.configuration);


        if (session.configuration.server_mode) {
            console.log("Using existing server on port %d", session.configuration.server_port);
            // make VS Code connect to debug server
            return new vscode.DebugAdapterServer(session.configuration.server_port);
        } else {
            console.log("Using executable: ", executable);
            return executable;
        }
    }

    dispose() {
        // stop server?
    }
}
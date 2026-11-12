import CoreML
import Foundation

private struct MotionRequest: Decodable {
    let audioBase64: String
}

private struct MotionResponse: Encodable {
    let framesBase64: String
    let fps: Int
    let frameCount: Int
}

private struct MotionReady: Encodable {
    let type = "ready"
    let models = ["lam", "foton"]
}

private struct MotionError: Encodable {
    let error: String
}

private enum AvatarMotionFailure: Error, LocalizedError {
    case invalidArguments
    case invalidAudio
    case invalidModelOutput(String)

    var errorDescription: String? {
        switch self {
        case .invalidArguments:
            return "Expected --lam and --foton model paths."
        case .invalidAudio:
            return "Audio motion input must contain exactly 48,000 Float32 samples."
        case .invalidModelOutput(let name):
            return "The local \(name) model returned an unexpected blendshape tensor."
        }
    }
}

private let sampleCount = 48_000
private let blendshapeCount = 52
private let outputFrameCount = 180

private func modelPath(_ name: String, in arguments: [String]) throws -> URL {
    guard let index = arguments.firstIndex(of: name), arguments.indices.contains(index + 1) else {
        throw AvatarMotionFailure.invalidArguments
    }
    return URL(fileURLWithPath: arguments[index + 1], isDirectory: true)
}

private func makeInput(_ samples: [Float]) throws -> MLMultiArray {
    let input = try MLMultiArray(shape: [1, NSNumber(value: samples.count)], dataType: .float16)
    let values = input.dataPointer.bindMemory(to: Float16.self, capacity: samples.count)
    for index in samples.indices {
        values[index] = Float16(samples[index])
    }
    return input
}

private func predict(_ model: MLModel, samples: [Float], name: String, expectedFrames: Int) throws -> MLMultiArray {
    let input = try makeInput(samples)
    let provider = try MLDictionaryFeatureProvider(dictionary: ["audio_waveform": input])
    let prediction = try model.prediction(from: provider)
    guard let output = prediction.featureValue(for: "blend")?.multiArrayValue,
          output.count == expectedFrames * blendshapeCount,
          output.dataType == .float16 else {
        throw AvatarMotionFailure.invalidModelOutput(name)
    }
    return output
}

private func encodeFrames(_ foton: MLMultiArray, lam: MLMultiArray) -> String {
    let fotonValues = foton.dataPointer.bindMemory(to: Float16.self, capacity: foton.count)
    let lamValues = lam.dataPointer.bindMemory(to: Float16.self, capacity: lam.count)
    var frames = [Float](repeating: 0, count: outputFrameCount * blendshapeCount)
    for frame in 0..<outputFrameCount {
        let seconds = Double(frame) / 60.0
        let lamPosition = (seconds - 1.0) * 30.0
        let lamIsAvailable = lamPosition >= 0 && lamPosition <= 59
        let lamBefore = lamIsAvailable ? Int(lamPosition) : 0
        let lamAfter = min(59, lamBefore + 1)
        let lamBlend = Float(lamPosition - Double(lamBefore))

        for channel in 0..<blendshapeCount {
            let fotonValue = Float(fotonValues[frame * blendshapeCount + channel])
            let value: Float
            if lamIsAvailable {
                let start = Float(lamValues[lamBefore * blendshapeCount + channel])
                let end = Float(lamValues[lamAfter * blendshapeCount + channel])
                let lamValue = start + (end - start) * lamBlend
                value = (fotonValue + lamValue) * 0.5
            } else {
                value = fotonValue
            }
            frames[frame * blendshapeCount + channel] = min(1, max(0, value))
        }
    }

    var bytes = Data(count: frames.count * MemoryLayout<Float>.size)
    bytes.withUnsafeMutableBytes { destination in
        guard let pointer = destination.bindMemory(to: Float.self).baseAddress else { return }
        for index in frames.indices {
            pointer[index] = frames[index]
        }
    }
    return bytes.base64EncodedString()
}

private func writeJSON<T: Encodable>(_ value: T) {
    do {
        var data = try JSONEncoder().encode(value)
        data.append(0x0A)
        FileHandle.standardOutput.write(data)
    } catch {
        FileHandle.standardError.write(Data("Could not encode local avatar motion response: \(error)\n".utf8))
    }
}

private func run() throws {
    let arguments = CommandLine.arguments
    let lamURL = try modelPath("--lam", in: arguments)
    let fotonURL = try modelPath("--foton", in: arguments)
    let configuration = MLModelConfiguration()
    configuration.computeUnits = .cpuAndGPU

    let lamModel = try MLModel(contentsOf: lamURL, configuration: configuration)
    let fotonModel = try MLModel(contentsOf: fotonURL, configuration: configuration)
    let warmupSamples = [Float](repeating: 0, count: sampleCount)
    _ = try predict(fotonModel, samples: warmupSamples, name: "Foton", expectedFrames: 180)
    _ = try predict(lamModel, samples: Array(warmupSamples.suffix(32_000)), name: "LAM", expectedFrames: 60)
    writeJSON(MotionReady())

    while let line = readLine() {
        do {
            guard let requestData = line.data(using: .utf8) else {
                throw AvatarMotionFailure.invalidAudio
            }
            let request = try JSONDecoder().decode(MotionRequest.self, from: requestData)
            guard let audioData = Data(base64Encoded: request.audioBase64),
                  audioData.count == sampleCount * MemoryLayout<Float>.size else {
                throw AvatarMotionFailure.invalidAudio
            }
            let samples = audioData.withUnsafeBytes { raw -> [Float] in
                let pointer = raw.bindMemory(to: Float.self)
                return Array(pointer)
            }
            let foton = try predict(fotonModel, samples: samples, name: "Foton", expectedFrames: 180)
            let lamSamples = Array(samples.suffix(32_000))
            let lam = try predict(lamModel, samples: lamSamples, name: "LAM", expectedFrames: 60)
            writeJSON(MotionResponse(framesBase64: encodeFrames(foton, lam: lam), fps: 60, frameCount: 180))
        } catch {
            writeJSON(MotionError(error: error.localizedDescription))
        }
    }
}

do {
    try run()
} catch {
    writeJSON(MotionError(error: error.localizedDescription))
    exit(EXIT_FAILURE)
}

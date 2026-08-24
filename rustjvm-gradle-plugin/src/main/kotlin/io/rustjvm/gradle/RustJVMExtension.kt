package io.rustjvm.gradle

import org.gradle.api.Action
import org.gradle.api.model.ObjectFactory
import java.io.File
import javax.inject.Inject

/** Configuration for the RustJVM Gradle plugin:
 *
 * ```kotlin
 * rustjvm {
 *     port = 8080
 *     sourceDir = file("src/main/java")
 *     hotReload = true
 *     docker { imageName = "myapp" }
 * }
 * ```
 */
open class RustJVMExtension @Inject constructor(objects: ObjectFactory, projectDir: File) {

    /** Development server port. */
    var port: Int = 8080

    /** Java source tree served by RustJVM. */
    var sourceDir: File = File(projectDir, "src/main/java")

    /** Watch sources and hot-swap on change (LiveRust). */
    var hotReload: Boolean = true

    /** The rustjvm CLI executable (PATH resolution by default). */
    var executable: String = "rustjvm"

    val docker: DockerConfig = objects.newInstance(DockerConfig::class.java)

    fun docker(action: Action<DockerConfig>) = action.execute(docker)
}

open class DockerConfig @Inject constructor() {
    var baseImage: String = "debian:bookworm-slim"
    var imageName: String = "rustjvm-app"
}

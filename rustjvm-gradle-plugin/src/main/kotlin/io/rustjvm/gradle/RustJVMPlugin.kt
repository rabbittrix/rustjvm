package io.rustjvm.gradle

import org.gradle.api.DefaultTask
import org.gradle.api.Plugin
import org.gradle.api.Project
import org.gradle.api.tasks.TaskAction

class RustJVMPlugin : Plugin<Project> {
    override fun apply(project: Project) {
        val ext = project.extensions.create(
            "rustjvm",
            RustJVMExtension::class.java,
            project.objects,
            project.projectDir,
        )

        project.tasks.register("rustjvmRun", RustJVMRunTask::class.java) {
            it.group = "rustjvm"
            it.description = "Run the app on the RustJVM runtime with hot reload."
            it.extension = ext
        }
        project.tasks.register("rustjvmBuild", RustJVMCliTask::class.java) {
            it.group = "rustjvm"
            it.description = "Validate routes and the DI graph; write the build manifest."
            it.extension = ext
            it.subcommand = "build"
        }
        project.tasks.register("rustjvmRoutes", RustJVMCliTask::class.java) {
            it.group = "rustjvm"
            it.description = "Print the route table and exit."
            it.extension = ext
            it.subcommand = "routes"
        }
        project.tasks.register("rustjvmDocker", RustJVMDockerTask::class.java) {
            it.group = "rustjvm"
            it.description = "Generate a minimal Dockerfile and build the image."
            it.extension = ext
        }
    }
}

/** Runs `rustjvm run` (blocking, with LiveRust hot reload). */
abstract class RustJVMRunTask : DefaultTask() {
    lateinit var extension: RustJVMExtension

    @TaskAction
    fun run() {
        project.exec {
            it.commandLine(
                extension.executable, "run",
                "--src", extension.sourceDir.absolutePath,
                "--port", extension.port.toString(),
            )
        }
    }
}

/** Generic single-subcommand task (build, routes). */
abstract class RustJVMCliTask : DefaultTask() {
    lateinit var extension: RustJVMExtension
    var subcommand: String = "build"

    @TaskAction
    fun run() {
        project.exec {
            it.commandLine(
                extension.executable, subcommand,
                "--src", extension.sourceDir.absolutePath,
            )
        }
    }
}

/** Generates the Dockerfile and, when docker is available, builds the image. */
abstract class RustJVMDockerTask : DefaultTask() {
    lateinit var extension: RustJVMExtension

    @TaskAction
    fun run() {
        val outDir = project.layout.buildDirectory.dir("rustjvm").get().asFile
        outDir.mkdirs()
        val dockerfile = outDir.resolve("Dockerfile")
        dockerfile.writeText(
            """
            FROM ${extension.docker.baseImage}
            WORKDIR /app
            COPY rustjvm-app /app/rustjvm-app
            EXPOSE ${extension.port}
            HEALTHCHECK --interval=5s --timeout=1s \
              CMD ["/app/rustjvm-app", "healthcheck"] || exit 1
            ENTRYPOINT ["/app/rustjvm-app"]
            """.trimIndent() + "\n",
        )
        logger.lifecycle("Wrote ${dockerfile.absolutePath}")
        logger.lifecycle("Build with: docker build -t ${extension.docker.imageName} ${outDir.absolutePath}")
    }
}

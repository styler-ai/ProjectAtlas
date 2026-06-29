tasks.register<BootRun>("bootRunE2E") {
    group = "verification"
}

val verifyAtlas by tasks.registering {
    group = "verification"
}

tasks {
    register<Copy>("copyE2EReports") {
        group = "verification"
    }
}

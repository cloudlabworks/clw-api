# Gateway restart from inside its own job

A job that the gateway itself was running tried to restart the gateway. The restart
killed the job halfway, the gateway never came back, and the widget service stayed
down until someone restarted it by hand.

Restart the gateway from a separate shell, then check its status.

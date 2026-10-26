## uic-modeler-mod15: circles and arcs with an exact description (modeling/curve.rs, modeling/messages.rs)

uic-curve-note-dropped = A circle or arc lost its exact description: its points are not on one circle any more. The curve itself stays.
uic-curve-note-angles = The start and end angle of an arc were updated. Its center and radius stay.
uic-curve-note-kind = A curve is now { $kind ->
        [arc] an arc
       *[other] a circle
    }. Its center and radius stay.
uic-curve-note-not-made = No curve was made because an edge is in a curve already. The circle description is left out.
uic-curve-refused-no-curve = The curve does not exist.
uic-curve-refused-no-description = This curve has no exact circle, so its segments cannot be set.
uic-curve-refused-range = The number of segments must be from 3 to 1,000.
uic-curve-refused-shared = Another edge meets the curve inside, so the curve cannot be rebuilt.
uic-curve-refused-plane = A face on another plane uses the curve, such as the wall of a pushed circle. The curve cannot be rebuilt.
uic-curve-refused-loop = A face uses the curve in a way that cannot be rebuilt.
uic-curve-refused-invalid = The new segments would make the model invalid, for example edges that are too short.
uic-curve-refused-unknown-kind = This circle has a kind that this program does not know, so it does not change it.
uic-curve-note-unknown-kind = A circle of a kind that this program does not know lost its description, because the curve changed.
uic-curve-note-broken = A circle or arc lost its exact description, because its edges are not one chain any more. The curve itself stays.

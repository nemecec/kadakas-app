"""Stepping stones: a path object. Stones sit along the path at an even spacing.

geometry.points is the list of path points, and geometry.closed tells if the path is a loop.
walk() gives a point and the direction of the path every `spacing` mm.
"""


def reset(params, geometry):
    shapes = []
    side = 1
    for x, y, angle in walk(geometry.points, params.spacing, params.start_offset, geometry.closed):
        # A zigzag moves each stone to the left or the right of the path, in turn.
        cx = x - math.sin(angle) * params.zigzag * side
        cy = y + math.cos(angle) * params.zigzag * side
        side = -side
        half = params.size / 2
        if params.stone == "Round":
            shapes.append(circle([cx, cy], half, pen=params.color, fill=params.color))
        else:
            corners = [
                rotate([cx + dx, cy + dy], angle, [cx, cy])
                for dx, dy in ((-half, -half), (half, -half), (half, half), (-half, half))
            ]
            shapes.append(polyline(corners, closed=True, pen=params.color, fill=params.color))
    return shapes

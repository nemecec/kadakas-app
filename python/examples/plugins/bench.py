"""Bench: a point object. The plan view shows the seat, the slats and an optional back rest.

The app calls reset() each time a parameter changes. geometry.at is the center of the bench and
geometry.rotation is its angle in radians. The helpers polyline(), line() and rotate() are
available without an import.
"""


def reset(params, geometry):
    at = geometry.at
    half_length, half_depth = params.length / 2, params.depth / 2

    def place(x, y):
        return rotate([at[0] + x, at[1] + y], geometry.rotation, at)

    seat = [
        place(-half_length, -half_depth),
        place(half_length, -half_depth),
        place(half_length, half_depth),
        place(-half_length, half_depth),
    ]
    shapes = [polyline(seat, closed=True, pen=params.color)]
    slats = int(params.slats)
    for i in range(1, slats):
        y = -half_depth + params.depth * i / slats
        shapes.append(line(place(-half_length, y), place(half_length, y), pen=params.color))
    if params.back:
        thickness = min(60.0, params.depth / 6)
        back = [
            place(-half_length, half_depth),
            place(half_length, half_depth),
            place(half_length, half_depth + thickness),
            place(-half_length, half_depth + thickness),
        ]
        shapes.append(polyline(back, closed=True, pen=params.color, fill=params.color))
    return shapes

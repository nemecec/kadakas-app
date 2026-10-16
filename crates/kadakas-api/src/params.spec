describe
  method str # a method name: returns the description and the JSON Schema of its parameters
  schemas bool # true: add method_schemas, the JSON Schema of every method
  schema bool # true: add command_schema, the JSON Schema of the commands of execute

execute
  command! command # a command: {op, ...}; see describe.command_examples

undo

redo

document.summary

document.get

document.load
  document! obj # a document as document.get returns it
  history any # versions of the document (a document with a history field brings its own)

file.save
  path! str # the file; the extension decides the form unless format is given
  format enum(container,json) # container (a ZIP file, the default) or json (one readable file)
  allow_missing str # the consent token of a refusal because parts of the file cannot be found
  allow_overwrite str # the consent token of a refusal because another program changed the file

file.open
  path! str

objects.list
  layer layer # only objects on this layer (ID or name)
  class class # only objects of this class (ID or name)
  kind str # only objects of this kind, for example rect or plant

objects.get
  id! object
  sources bool # show the import tags of the sources of a site model (data.import_sources)

selection.get

selection.set
  ids! object[]

render.svg
  width int # pixels
  region num[] # [x0, y0, x1, y1] in drawing coordinates (mm): draw only this part, and draw the units of the paved areas in it one by one when there are at most 60,000; above that the paved areas are a preview (marked in the picture)
  show_check bool # mark the findings of unit_pattern.check (overlaps, small pieces, joints, holes) on the paved areas
  compare obj # {from, to, mode?, hide_unchanged?, ...}: draw the changes since a version

render.png
  view str # a name from view.list; default the current view
  width int = 800 # pixels, 16 to 2048
  height int = 600 # pixels, 16 to 2048
  projection enum(ortho,perspective)
  mode enum(shaded,wireframe,hidden_line,dashed_hidden_line)
  render_style ref # a render style of the document (ID or name) or a built-in one such as Realistic
  samples int # 1 to 3 (anti-aliasing)
  compare obj # {from, to, mode?, hide_unchanged?, ...}: draw the changes since a version

view.list

view.set
  view str # a name from view.list, or the name of a saved view (its camera only)
  projection enum(ortho,perspective)
  mode enum(shaded,wireframe,hidden_line,dashed_hidden_line)

view.get

view.zoom
  ids object[] # objects to frame
  box num[] # [x0, y0, x1, y1] in drawing coordinates (mm)
  at num[] # [x, y] or [x, y, z]: the new middle of the view
  scale num # px per mm of the plan, 0.0001 to 100
  margin num = 0.1 # space around ids or box, as a fraction of its size on each side (0 to 10)
  @one_of ids,box,at

view.pan
  by num[] # [dx, dy] (3D also dz) in drawing mm, east and north
  to num[] # [x, y] (3D also z): the new middle of the view
  @one_of by,to

view.camera
  eye! num[] # [x, y, z] in drawing mm
  target! num[] # [x, y, z] in drawing mm
  up num[] # must point upward (z above 0): the camera has no roll
  fov num # degrees; the camera has a fixed 45
  projection enum(ortho,perspective)

view.highlight
  ids object[] # objects to mark with an outline glow
  box num[] # [x0, y0, x1, y1] in drawing coordinates (mm)
  at num[] # [x, y] or [x, y, z]: a marker at this place
  text str # a short text, at most 80 characters, one line
  color enum(agent,red,blue,green,orange) = agent
  until str|int # next_change, dismissed (default) or a time in ms (1 to 3600000)
  @one_of ids,box,at

view.highlight_clear
  id int # one pointer; all when left out

report.plants

report.areas

import.file
  path! str
  format enum(dxf,dwg,shp,geojson,ifc) # default: from the extension
  options obj # see describe.io for the options of each format

export.file
  path! str
  format enum(dxf,geojson,dwg,ifc,gltf,glb,obj,pdf,sheet_svg,sheet_png) # default: from the extension; the sheet formats need format
  options obj # see describe.io for the options of each format
  overwrite bool # replace an existing file; a server for agents needs it

import.update
  path! str
  drawing str # the lineage; needed when several drawings were imported and the file does not say which
  old str # the old revision, when the document has no update data and no usable link
  layers str[] # only entities on these DXF layers; the other layers stay at the old revision
  tolerance num # largest distance of a moved entity, in units of the drawing; default automatic
  dry_run bool # true: no pending update is kept
  limit int = 200 # most changes listed, at most 1000
  options obj # import options of the new file such as hatches or base: auto|link|none

import.apply
  update_id! str
  accept! str|str[] # "all" or a list of change ids
  reject str[] # change ids
  conflicts enum(skip,overwrite) = skip # skip keeps the edited objects of a change that the person edited

plant_style.list

plant_style.get
  id! ref # the ID or the name of the style

plant_style.create
  name! str
  botanical_name str
  common_name str
  id_code str
  category enum(tree,shrub,perennial,grass,groundcover)
  spread num # mm
  height num # mm
  graphic enum(circle,cloud,star,spokes,rosette,clover)
  form enum(tree,columnar,conical,spreading,shrub_mound,weeping,palm,columnar_conifer,multi_stem,hedge) # the 3D form
  fill color
  pen color
  price num
  size str
  container str
  root str
  custom map(str) # {key: text}
  locked str[] # fields that a plant may not override
  quantity str # how the plants are counted in the schedule
  trunk_diameter num # mm
  protection_zone bool
  rpz_factor num|null
  rpz_cap num|null
  plan_symbol ref|null # a symbol
  image ref|null # the picture resource
  maintenance obj # {pruning, fertilizer, interval, hours_per_year, notes}
  irrigation obj # {irrigated, water_need, plant_factor, hydrozone}

plant_style.update
  id! ref
  name str # renames
  @include plant_style.create

plant_style.duplicate
  id! ref
  name str

plant_style.delete
  id! ref
  reassign ref # another plant style that takes the plants

plant_style.import_csv
  csv! str
  pictures_dir str # folder that the picture file names of an image column are relative to
  unit enum(mm,cm,m) # for spread and height columns without a unit
  mapping map(str) # {field: header}
  on_duplicate enum(skip,update) = skip

plant_style.export_csv
  ids ref[]

plant_style.sample_csv

plants.place
  mode! enum(single,path,grid,triangular,fill,poly,hedgerow)
  style ref # ID or name
  overrides obj # {field: value}
  variation obj # {seed?, size? (0 to 0.9), rotation? (bool)}; the same seed gives the same plants
  layer layer
  class class
  tag bool # true adds a plant tag
  on_terrain bool # true stands each plant on the ground of the site model that covers it
  source object # a polyline, rect, circle, landscape area or hardscape to use as the path, boundary or rectangle
  at point # single
  path vertex[] # path, hedgerow
  closed bool # path, hedgerow
  spacing num|str # mm, or "spread" (one canopy diameter); path, grid, triangular, fill, hedgerow
  spacing_x num|str # grid
  spacing_y num|str # grid
  count int # path, fill
  min point # grid, triangular
  max point # grid, triangular
  margin num # grid, triangular, fill
  boundary vertex[] # fill
  density num # fill: plants per m2
  pattern enum(triangular,rectangular) # fill
  holes point[][] # fill: areas that stay empty
  points point[] # poly
  rows int # hedgerow, 1 to 20
  row_spacing num # hedgerow
  stagger bool # hedgerow

plants.set_style
  ids! object[]
  style ref # ID or name
  overrides obj
  unlink bool

report.plant_schedule
  format enum(csv)

report.existing_trees

plants.drape
  ids object[] # plants and existing trees; default all
  release bool # true removes the stored height

plant_tag.set
  ids! object[] # plant tags
  levels int # 1 to 3
  bubble bool

plant_style.set_picture
  id! ref
  path str # a PNG or JPEG file
  base64 str # the file bytes as base64 text
  @one_of path,base64
  name str # the file name to remember, for base64

plant_style.clear_picture
  id! ref

plant_style.get_picture
  id! ref
  thumbnail bool # true adds thumbnail_base64
  bytes bool # true adds base64, the original file


record_format.list

record_format.get
  id! ref

record_format.create
  name! str
  fields! obj[] # [{name, type, default?, unit?, choices?}]; type is text, integer, number, boolean, dimension, area, choice, date, time, volume or angle

record_format.update
  id! ref
  name str
  fields obj

record_format.delete
  id! ref

record.attach
  ids! id[]
  record! str # ID or name
  values obj

record.detach
  ids! id[]
  record! str

record.set
  ids! id[]
  record! str
  values! obj

record.get
  id! object

data_manager.list

data_manager.get
  id! ref

data_manager.create
  name! str
  template str
  entities any
  rules any

data_manager.update
  id! ref
  name str
  entities any
  rules any

data_manager.delete
  id! ref

data_manager.check
  id! ref

data_manager.evaluate
  id! ref
  ids id[]
  limit int

data_manager.apply
  id! ref

data_manager.properties

worksheet.list

worksheet.templates

worksheet.create
  name! str
  template str
  cells obj
  databases any

worksheet.get
  id! ref

worksheet.rename
  id! ref
  name! str

worksheet.delete
  id! ref

worksheet.set_cell
  id! ref
  address! str
  input! str
  decimals int
  unit str

worksheet.set_cells
  id! ref
  cells! obj

worksheet.set_database
  id! ref
  index int
  database! obj

worksheet.remove_database
  id! ref
  index! int

worksheet.evaluate
  id! ref

worksheet.export
  id! ref
  format! enum(csv,xlsx)
  path str
  overwrite bool

worksheet.place
  id! ref
  at! point # top left corner
  text_height num = 3.5 # mm, default 3.5
  layer layer
  class class

worksheet.fit
  ids object[]

material.list

material.get
  id! ref

material.create
  name! str
  base_color color # [r,g,b] or [r,g,b,a]
  texture str|obj|null # null | "grass" | {procedural: grass|gravel|pavers|mulch|water|brick|timber|noise, seed?} | {png_base64} | {jpeg_base64} | {path}
  mapping obj # {kind: planar|box|cylindrical, scale: mm of one repeat as a number or [u, v], rotation: degrees, offset: [x, y] mm}
  roughness num # 0 to 1
  metallic num # 0 to 1
  opacity num # 0 to 1; below 1 is see-through, for water and glass

material.update
  id! ref
  name str # renames
  @include material.create

material.duplicate
  id! ref
  name str

material.delete
  id! ref

material.add_defaults

material.import_texture
  id! ref
  path str
  png_base64 str
  jpeg_base64 str
  @one_of path,png_base64,jpeg_base64
  max_side int = 1024 # default 1024, at most 2048

material.assign
  material! ref|null # a material (ID or name), or null to clear the slot
  object object # a target: one object
  objects object[] # or several objects
  style ref # or a plant style or hardscape style (ID or name)
  class class # or a class
  slot str # a part: trunk, canopy, surface, component:N

material.slots

render_style.list

render_style.get
  id! ref

render_style.create
  name! str
  mode enum(shaded,wireframe,hidden_line,dashed_hidden_line)
  edges bool # draw edges on shaded surfaces
  shadows bool # needs sun
  sun bool # light with the sun of the document, see sun.set
  textures bool
  background color # [r,g,b,a]
  line_weight num # mm on paper, for hidden line

render_style.update
  id! ref
  name str # renames
  @include render_style.create

render_style.duplicate
  id! ref
  name str

render_style.delete
  id! ref

render_style.set_active
  id ref
  null any
  @one_of id,null

sun.set
  latitude num # degrees north
  longitude num # degrees east
  from_georef bool # take latitude and longitude from the georeference
  date any # "YYYY-MM-DD"
  time any # "HH:MM" local clock time
  tz_offset any # standard time, hours from UTC; default: the nominal zone of the longitude
  dst bool # true when summer time is in force: the clock is one hour ahead of tz_offset
  north_angle num # degrees clockwise from the model +Y axis to true north; from_georef uses the rotation of the georeference and the meridian convergence of its coordinate system
  off bool # true removes the sun

sun.get
  times any # list of "HH:MM" for a sun path on the same date

render.hidden_line
  view any
  projection any
  width num
  height num # px, as render.png
  dashed bool # also return the covered edges, dashed and gray
  mesh_edges bool = true # add silhouettes and creases of the surfaces; default true
  segments bool # also return the segments

gis.sources

gis.cache.clear

gis.georef.set
  crs! str # 'EPSG:3067' or WKT/PROJ text; a projected CRS in meters
  origin point # [easting, northing] in meters, default [0, 0]
  rotation num = 0 # radians, default 0

gis.coords
  x! num
  y! num

gis.reproject
  crs! str # 'EPSG:32635'
  dry_run bool # true: only the report
  origin point # [easting, northing] in the new CRS for the model origin; default: the old origin converted and rounded to a meter

gis.wms.capabilities
  url! str
  wait bool

gis.wms.add
  url! str
  layers! str[] # the WMS layers
  styles str[]
  crs str
  format str
  transparent bool = true
  version str
  bbox num[] # [min x, min y, max x, max y] in model mm; default: the drawing plus 5 percent
  max_px int = 2048 # at most 4096
  name str
  opacity num # 0 to 1
  layer layer # the design layer
  class class
  wait bool

gis.wms.refresh
  id! object
  bbox num[]
  max_px int
  wait bool

gis.wmts.capabilities
  url! str
  wait bool

gis.wmts.add
  url! str
  layer! str # the WMTS layer
  matrix_set str
  style str
  format str
  bbox num[] # [min x, min y, max x, max y] in model mm
  max_px int
  name str
  opacity num
  layer_name layer # the design layer
  design_layer layer # the design layer
  class class
  wait bool

gis.wfs.capabilities
  url! str
  wait bool

gis.wfs.import
  url! str
  type_name! str
  crs str
  bbox num[] # [min x, min y, max x, max y] in model mm
  max_features int = 5000 # at most 50000
  format enum(geojson,gml)
  options obj # as in import.file: layer, class_field, origin, ...
  caps bool # false: skip GetCapabilities
  wait bool


gis.geoimage.import
  path! str
  crs str # when the picture has none
  max_px int = 2048 # default 2048
  name str
  opacity num
  layer layer
  class class

gis.job.status
  job! str
  wait num = 0 # seconds to wait

gis.job.cancel
  job! str

massing.add
  footprint! vertex[] # a closed outline; curves are allowed like in polylines
  height num = 3000 # eave height in mm
  floors int
  floor_height num = 3000 # mm
  roof enum(flat,gable,hip)
  pitch num = 30 # roof pitch in degrees
  ridge_angle num # radians; default: along the long side
  base num # height of the base above the layer in mm; default: on the site model of the layer, else 0
  name str
  layer layer
  class class
  parent object # a group

massing.set
  id object
  ids object[]
  @one_of id,ids
  footprint vertex[]
  height num
  floors int|null # null for none
  floor_height num
  roof enum(flat,gable,hip)
  pitch num
  ridge_angle num|null # null for the default
  base num|null # null: on the site model
  name str

massing.get
  id! object

massing.from_shapes
  ids object[] # default: the selection
  height_field str # default: height, building_height, building:height, ...
  floors_field str # default: floors, levels, building:levels, ...
  height_scale num = 1000 # mm per unit of the height field
  name_field str
  default_height num = 3000 # mm, used when the shape has no height and no floors
  floor_height num
  roof enum(flat,gable,hip)
  pitch num
  require_height bool # leave out shapes without a height
  delete_source bool = true
  dry_run bool

massing.follow_sources
  ids object[] # default: all models with a link to their shape


massing.report
  ids id[]
  format str

extrude.add
  profile! vertex[] # a closed outline, curves allowed like in polylines
  holes any # closed outlines inside the profile
  height! num # mm, not zero; negative extrudes downward
  base num = 0 # height of the profile above the layer in mm, default 0
  name str
  layer layer
  class class
  parent any # a group

extrude.set
  id object
  ids id[]
  @one_of id,ids
  profile vertex[]
  holes any
  height num
  base any
  name str

extrude.get
  id! object

extrude.from_shapes
  ids id[] # default: the selection
  height! num = 1000 # mm, default 1000
  base num = 0 # mm, default 0
  delete_source bool # default true: the 2D shape becomes the solid; false keeps it
  dry_run bool

sweep.add
  profile! vertex[] # x is the distance from the axis, y the position along it; a closed profile makes a solid with end caps, an open one a surface
  closed bool = true # default true
  origin point # [x, y, z] of a point of the axis, mm; z is measured from the layer
  axis any # [x, y, z] direction, default [0, 0, 1]
  rotation num # degrees, the angle of the profile plane around the axis at the start
  sweep_angle num = 360 # degrees, default 360, at most 3600; more than 360 needs a pitch at least as large as the profile along the axis, or the turns cross each other
  segment_angle num = 10 # degrees, default 10, from 0.1 to 180: the smaller, the rounder
  pitch num = 0 # rise along the axis for each full turn in mm, default 0; a spiral
  name str
  layer layer
  class class
  parent any

sweep.set
  id object
  ids id[]
  @one_of id,ids
  profile vertex[]
  closed bool
  origin point
  axis any
  rotation num
  sweep_angle any
  segment_angle any
  pitch num
  name str

sweep.get
  id! object

sweep.from_shapes
  ids id[] # default: the selection
  axis_id id # a line object, or a polyline with two vertices: the profile turns around it, in the plane of the plan; the axis is horizontal and the line stays
  axis_at any # [x, y]: a vertical axis at this point instead
  sweep_angle any # default 360
  segment_angle any # default 10
  pitch num = 0 # mm per turn, default 0
  delete_source bool = true # default true
  dry_run bool

polygon3d.add
  points! point[] # [[x, y, z]]: z is measured from the layer, in mm
  closed bool = true # a closed polygon needs 3 points, an open one 2
  fill color # a filled closed polygon is a face in 3D views and renders
  name str # in the object data
  layer layer
  class class
  parent object # a group

polygon3d.set
  id object
  ids id[]
  @one_of id,ids
  points any
  closed bool
  raise num # mm added to the z of every vertex

polygon3d.get
  id! object

hardscape_style.list

hardscape_style.get
  id! ref

hardscape_style.create
  name! str
  material str # surface material
  fill color
  pen color
  thickness num = 60 # surface layer, mm
  components obj[] # [{name, material, thickness, color?}], layers below the surface from top to bottom
  border obj|null # {width, material, color?, sides? (both|left|right; a path only)}
  joints obj|null # {kind (running_bond|stack|herringbone|basket_weave), unit_length, unit_width, spacing, angle, color?}
  custom map(str) # {key: text}

hardscape_style.update
  id! ref
  name str # renames
  @include hardscape_style.create

hardscape_style.duplicate
  id! ref
  name str

hardscape_style.delete
  id! ref
  reassign any

landscape_area_style.list

landscape_area_style.get
  id! ref

landscape_area_style.create
  name! str
  mix obj[] # [{plant (plant style ID or name), percent? (default 100, all together at most 100), spacing? (mm) | density? (plants per m2), min_count?}]
  fill color
  pen color
  line_weight num # mm
  show_plants bool # draw the plants in 2D
  fields map(str) # data fields such as hydrozone

landscape_area_style.update
  id! ref
  name str # renames
  @include landscape_area_style.create

landscape_area_style.duplicate
  id! ref
  name str

landscape_area_style.delete
  id! ref
  reassign any

hardscape.add
  boundary vertex[] # a slab: the outline
  center vertex[] # a path: the center line (curves are allowed)
  @one_of boundary,center
  mode enum(slab,path) # slab keeps the outline; path makes a slab into a path
  width num = 1200 # path, mm
  join enum(miter,round,bevel) # path
  closed bool # path: a ring without ends
  align enum(center,left,right) # path: which edge of the path the line is
  offset num # path: mm, moves the path to the left of the line; negative to the right
  style ref # ID or name
  slope obj # {type: none} | {type: edges, elevations} | {type: cross, percent, angle_deg?} | {type: angle, degrees, angle_deg?} | {type: rise_run, rise, run, angle_deg?}
  elevation num # top surface above the layer; above the ground when draped
  drape bool # follow the site model that covers it
  site_modifier bool # act as a pad on the site model; the formation level is the top minus the build-up
  material str # when there is no style
  thickness num # when there is no style
  holes vertex[][] # open parts inside the boundary such as planting islands
  exclude object[] # objects whose outline becomes a hole
  origin point # where the joint pattern starts
  angle num # degrees: the angle of the joint pattern
  align_to_edge int # start the pattern at vertex n of the outline and run it along the edge to vertex n+1
  layer layer
  class class

hardscape.set
  id object
  ids object[]
  @one_of id,ids
  @include hardscape.add
  mode str # slab or path
  style ref|null # null removes it
  slope obj|null
  origin point|null # null clears it
  angle num|null # null clears it
  holes vertex[][]|null # replaces the old holes; [] or null clears them


hardscape.from_shapes
  ids id[] # default: the selection
  style ref # ID or name
  holes_from_inner bool = true # default true; false makes every shape a hardscape of its own
  delete_source bool # default false: the outlines stay
  dry_run bool

unit_pattern.layout
  id object # a closed polyline, rectangle, circle, landscape area or hardscape
  region obj # {outline: [vertices], holes?: [[vertices]]}
  @one_of id,region
  pattern obj # {bond, unit_length, unit_width, gap?} or a module; see unit_pattern.module_preset
  origin point # where the pattern starts
  angle num # degrees, counterclockwise
  align_to_edge int # start at vertex n of the outline and run along the edge to vertex n+1
  include enum(units,gaps)[] # default both
  offset int = 0 # a page of the lists
  limit int = 1000 # at most 20000; 0 gives the counts only
  tolerance num = 1 # curve flattening, mm
  sliver_share num = 0.25
  unfillable_share num = 0.05
  min_width num
  exclude_unfillable bool = false
  trim num = 0.1
  waste_percent num
  gap_length bool = false
  max_extra_joint_mm num = 2

unit_pattern.to_objects
  id object
  region obj
  @one_of id,region
  pattern obj
  origin point
  angle num
  align_to_edge int
  tolerance num = 1
  include enum(units,gaps)[] # default ["units"]
  layer layer # default: the layer of the object, else the active layer
  class class
  group bool = true
  name str # the name of the group

unit_pattern.module_preset
  preset! enum(hexagon,octagon_square,i_shape)
  side num # hexagon, octagon_square: mm
  across_flats num # hexagon: mm
  length num = 200 # i_shape: mm
  width num = 165 # i_shape: mm
  notch num = 25 # i_shape: mm
  gap num

unit_pattern.module_from_shapes
  ids object[] # default: the selection
  names str[] # one per shape
  a point
  b point

unit_pattern.module_from_block
  path! str
  block! str
  a point
  b point

report.unit_pattern
  ids object[] # default: every hardscape with a joint pattern
  pattern obj
  origin point
  angle num
  align_to_edge int
  sliver_share num = 0.25
  unfillable_share num = 0.05
  min_width num
  exclude_unfillable bool = false
  trim num = 0.1
  waste_percent num
  gap_length bool = false
  max_extra_joint_mm num = 2
  visible_only bool = false
  tolerance num = 1
  format enum(csv)

region.from_curves
  ids object[] # default: the selection
  tolerance num = 1 # mm, the widest gap that is closed
  create enum(hardscape,landscape_area,polyline)
  depths enum(even,odd,all)
  regions (int|point)[] # region indexes or points: make exactly these regions
  style ref # hardscape style, for create hardscape
  name str # landscape area name
  min_area_m2 num
  layer layer
  class class
  delete_source bool
  dry_run bool

hardscape.split
  id! object
  lines (object|vertex[])[] # replaces the split lines; [] removes them
  follow bool = true
  zones obj[] # [{index?, seed?, name?, pattern?, origin?, angle?, align_to_edge?, align_to_split?}]
  tolerance num = 1 # mm
  zone_units enum(cut,centroid) = cut

hardscape.course
  id! object
  courses obj[] # [{name, path | source | along, side?, offset?, closed?, orientation?, unit_length?, unit_width?, gap?, corner?, unit?, align_field?}]
  name str # the fields of one course directly in the params
  path vertex[]
  source object
  along str
  side enum(left,right,center,inside,outside)
  offset num
  closed bool
  orientation enum(soldier,stretcher)
  unit_length num
  unit_width num
  gap num
  corner enum(miter,butt,corner_unit)
  unit str
  align_field bool

hardscape.course_remove
  id! object
  names str[]
  all bool

hardscape.plan
  id! object
  outlines bool

hardscape.plan_clear
  id object
  ids object[]
  @one_of id,ids
  what enum(all,splits,zones,courses) = all

unit_pattern.check
  ids object[] # default: every hardscape with a joint pattern
  visible_only bool
  overlap_mm2 num = 1
  overlap_depth_mm num = 0.15
  min_joint_mm num
  max_joint_mm num
  min_share num = 0.05
  min_width_mm num
  size_tol_mm num = 0.05
  coverage_tol num = 0.02
  trim num = 0.1
  max_findings int = 200 # per kind, at most 10000
  pattern obj
  origin point
  angle num
  align_to_edge int
  tolerance num


landscape_area.add
  boundary! vertex[]
  name str
  style ref # ID or name
  holes any # [[vertices]], parts that stay empty and lie inside the boundary
  exclude any # [object IDs] whose outline becomes a hole: rect, circle, closed polyline, hardscape, landscape area
  show_plants bool
  drape bool
  layer layer
  class class

landscape_area.set
  id object
  ids id[]
  @one_of id,ids
  style ref # null removes it
  name str
  holes any
  exclude any
  show_plants bool
  drape bool

report.hardscape
  ids object[] # default: every hardscape
  format enum(csv)
  pattern bool # false leaves out the unit pattern block
  sliver_share num = 0.25
  unfillable_share num = 0.05
  min_width num
  exclude_unfillable bool = false
  trim num = 0.1
  waste_percent num
  gap_length bool = false
  max_extra_joint_mm num = 2
  visible_only bool = false
  tolerance num = 1

report.landscape_areas
  format str

report.plant_list
  format str

irrigation.outlet.add
  at! point
  model! str # a head of irrigation.catalog.get
  arc num = 360 # spray angle in degrees
  direction num = 0 # degrees, the middle of the arc, counterclockwise from +X
  radius num # mm; default: the radius of the model
  layer layer
  class class

irrigation.line.add
  points! point[] # 2 or more
  line_type enum(mainline,lateral,drip) = lateral
  size! str # nominal pipe size, for example "32"
  material! str # for example "PE"
  emitter_spacing num # mm, drip line
  emitter_flow num # l/h per emitter, drip line
  layer layer
  class class

irrigation.poc.add
  at! point
  static_pressure! num # kPa
  available_flow! num # l/min
  supply_loss num # kPa lost in the supply line at the available flow
  meter_loss num # kPa lost in the meter and backflow preventer at the available flow
  name str
  layer layer
  class class

irrigation.valve.add
  at! point
  zone! int # 1 or more
  kv num = 6 # flow factor in m3/h at 1 bar
  name str
  controller str # name of a controller
  station int
  layer layer
  class class

irrigation.controller.add
  at! point
  name! str
  stations int = 6 # default 6
  layer layer
  class class

irrigation.set open
  id object
  ids object[]
  @one_of id,ids
  # The other parameters are the fields of the object. outlet: model, arc, direction, radius,
  # zone. line: points, line_type, size, material, emitter_spacing, emitter_flow. poc: at, name,
  # static_pressure, available_flow. valve: at, zone, name, kv, controller, station. controller:
  # at, name, stations. null clears an optional field.
  model str
  arc num
  direction num
  radius num|null
  zone int|null
  points point[]
  line_type enum(mainline,lateral,drip)
  size str
  material str
  emitter_spacing num|null
  emitter_flow num|null
  at point
  name str|null
  static_pressure num
  available_flow num
  supply_loss num|null
  meter_loss num|null
  kv num
  controller str|null
  station int|null
  stations int

irrigation.catalog.get
  document_only bool

irrigation.catalog.set
  heads obj[] # lists as in irrigation.catalog.get
  pipes obj[]
  materials obj[]
  replace bool # drop the old document parts first

irrigation.catalog.import_csv
  kind! enum(heads,pipes,materials)
  text! str
  replace bool

irrigation.catalog.export_csv
  kind! enum(heads,pipes,materials)
  document_only bool

irrigation.calculate
  tolerance num = 50 # mm
  fittings num = 0 # percent added to the pipe losses
  use_site_model bool = true # the height of the site model counts, 9.81 kPa per m
  assign_zones bool = true # store the zone in the outlets, one undo step
  areas map(num) # {zone: m2}, real areas for the precipitation rate
  target_depth num = 10 # mm

irrigation.layout.fill
  boundary vertex[]
  object object # a rect, closed polyline, landscape area or hardscape
  @one_of boundary,object
  model! str
  spacing num # head-to-head, mm; default the radius of the model
  pattern enum(square,triangular)
  angle num # degrees
  inset num # mm from the edge
  arc num = 360
  place bool = true # add the outlets in one undo step
  layer layer # the layer of the new objects; default the active layer
  class class

irrigation.layout.edge
  path vertex[]
  object object # a polyline or area
  @one_of path,object
  closed bool # the path is a ring
  model! str
  spacing num # default the radius of the model; the heads are spread evenly at most this far apart
  arc num = 180
  side enum(left,right) = left # the way the heads face
  place bool = true
  layer layer
  class class

irrigation.layout.drip
  boundary vertex[]
  object object
  @one_of boundary,object
  spacing! num # mm between lines
  angle num # degrees
  inset num # mm
  size str = 16
  material str = PE
  emitter_spacing num
  emitter_flow num
  place bool = true
  layer layer
  class class

irrigation.report.zones
  target_depth num = 10 # mm
  areas map(num) # {zone: m2}
  tolerance num
  format enum(csv)

irrigation.report.pipes
  tolerance num
  format str

irrigation.report.pressure
  tolerance num
  format str

irrigation.report.materials
  tolerance num
  format str

irrigation.report.hydrozones
  eto num = 1000 # reference evapotranspiration in mm per year
  target_depth num
  tolerance num
  format enum(csv)

wall_style.create
  name! str
  thickness num = 300 # mm; with components it is their total
  material str
  components obj[] # [{name, material, thickness}], across the thickness
  coping obj|null # {height, overhang?, material?}
  color color

wall_style.list

wall_style.update
  id! ref
  name str # renames
  @include wall_style.create

wall_style.delete
  id! ref
  reassign ref

wall.add
  path! vertex[]
  closed bool
  thickness num = 300 # mm
  top obj # {mode: fixed, elevation} | {mode: terrain, offset} | {mode: slope, start, end}
  bottom enum(existing,proposed) # the ground that the bottom and a terrain top follow
  embed num # mm below the ground
  style ref # its thickness and coping win
  site object # a site model; default the one that covers the wall
  site_modifier bool # the wall is a retaining edge
  retains enum(left,right) = left
  backfill num = 1500 # mm the retained ground reaches behind the wall
  layer layer
  class class

wall.set
  id object
  ids object[]
  @one_of id,ids
  @include wall.add

wall.report
  ids object[]

grade.add
  a point
  b point
  path vertex[] # a curve with arcs; instead of a and b
  site_modifier bool # the grade changes the proposed surface
  width num = 1500 # mm
  elevation_a num
  elevation_b num
  slope obj # {percent: 2} | {ratio: 50} | {angle: 1.2}; positive rises from a to b
  snap enum(existing,proposed) # both elevations are read from the site model, live
  site object # a site model
  label any
  end_tags any
  layer layer
  class class

grade.set
  id! object
  a point
  b point
  path vertex[]
  site_modifier bool
  width num
  elevation_a num|null
  elevation_b num|null
  slope obj|null
  snap enum(existing,proposed)|null
  site object|null
  label any
  end_tags any

grade.get
  id! object

site.analysis
  id! object
  mode enum(off,slope,elevation)
  surface enum(existing,proposed)
  breaks num[] # class limits, ascending: slope in percent, elevation in mm; [] for the defaults
  colors color[] # one per band
  opacity num # 0 to 1, the 2D tint
  legend obj # {rect: [[x0, y0], [x1, y1]], title?, show_areas?}: adds a legend object on the layer of the site model

spot.add
  at! point
  site object
  show enum(existing,proposed,both)
  kind enum(spot,stake)
  prefix str
  name str # stake name
  decimals int = 2 # 0 to 4
  label_height num = 2.5 # mm on paper
  layer layer
  class class

spot.set
  id! object
  at point
  site object|null
  show enum(existing,proposed,both)
  kind enum(spot,stake)
  prefix str
  name str
  decimals int
  label_height num

spot.get
  id object
  at point
  site object
  @one_of id,at

section.add
  a! point
  b! point
  name str = A
  exaggeration num = 2 # vertical
  step num # mm between samples when the line leaves the model
  band num # mm: plants this near the line show
  walls bool
  hardscapes bool
  plants bool
  site object
  layer layer
  class class

section.set
  id! object
  a point
  b point
  name str
  exaggeration num
  step num
  band num
  walls bool
  hardscapes bool
  plants bool
  site object|null

section.profile
  id! object

section.place
  id! object
  at! point # where the start of the section goes
  layer layer # default the layer of the section; may be a sheet layer
  scale num # drawing scale denominator, for example 100

pad.add
  boundary! vertex[] # arcs allowed
  elevation! num # mm
  slope obj # {direction (radians the ground falls), percent}
  retaining_edge bool|obj # true or the settings of pad.retaining_edge
  layer layer
  class class

grade_limit.add
  boundary! vertex[] # 3 or more, arcs allowed
  layer layer # the modifier acts on the site models of its layer
  class class

pad.retaining_edge
  id! object # a pad
  enabled bool = true # false removes the wall
  thickness num = 300 # mm
  top_offset num = 0 # mm
  embed num = 0 # mm below the ground
  style ref # wall style
  class class

plugin.define
  name! str
  shape_type! enum(point,linear,rectangle,path)
  params obj[] # [{name, label?, type: number | length | text | bool | choice | color | record, default?, min?, max?, options?, fields?}]
  script! str # defines reset(params, geometry) and returns a list of shapes
  description str
  timeout_ms int = 5000 # 10 to 60000

plugin.update
  id object
  name str
  @one_of id,name
  new_name str
  shape_type enum(point,linear,rectangle,path)
  params obj[]
  script str
  description str
  timeout_ms int
  reset bool

plugin.delete
  id object
  name str
  @one_of id,name

plugin.list

plugin.get
  id object
  name str
  @one_of id,name

plugin.examples

plugin.install_example
  name! str

plugin.place
  definition! ref # an ID or a name
  params obj # {name: value}; missing values use the defaults
  layer layer
  class class
  name str
  at point # point: where
  rotation num # point: radians
  a point # linear
  b point # linear
  min point # rectangle
  max point # rectangle
  points point[] # path
  closed bool # path

plugin.set_params
  id! object
  params! obj

plugin.reset
  id object
  ids object[]
  definition ref
  @one_of id,ids,definition

plugin.object
  id! object

plugin.preview
  definition! ref
  params obj
  at point
  rotation num
  a point
  b point
  min point
  max point
  points point[]
  closed bool

script.info

script.run
  path str # must end in .py and be inside an allowed folder
  source str
  @one_of path,source
  args str[]

script.status
  run! str
  since int # line number

script.stop
  run! str

script.list

plugins.info

plugins.list

plugins.install
  path! str

plugins.remove
  plugin! ref

plugins.place
  plugin! ref
  params obj
  layer layer
  class class
  name str
  at point
  rotation num
  a point
  b point
  min point
  max point
  points point[]
  closed bool

plugins.set_params
  id! object
  params! obj

plugins.reset
  id object
  ids object[]
  plugin ref
  @one_of id,ids,plugin

plugins.object
  id! object

plugins.preview
  plugin! ref
  params obj
  at point
  rotation num
  a point
  b point
  min point
  max point
  points point[]
  closed bool

plugins.tool_step
  plugin! ref
  event! obj # {type: start | move | click | finish, at?: [x, y]}
  modifiers obj # {shift?, ctrl?, alt?}
  state any # what the last step returned; null at the start
  params obj # tool options
  commit bool = true
  layer layer
  class class
  name str


section_viewport.line_add
  a! point
  b! point
  name str = A
  look enum(left,right) = left
  mode enum(cut,elevation)
  depth num # far clip in mm; default no limit
  beyond bool = true
  layer layer
  class class

section_viewport.line_set
  id! object
  a point
  b point
  name str
  look enum(left,right)
  mode enum(cut,elevation)
  depth num|null # null removes the limit
  beyond bool

section_viewport.create
  sheet! sheet
  line! object # a section line
  layers layer[] # design layers that the viewport shows; default all
  min point # the frame on the sheet, mm
  max point
  at point
  size point
  scale num|enum(fit) = fit # a denominator or "fit"
  center point # [distance along the line, height] in mm
  name str
  show_frame bool
  cut_weight num = 0.5 # mm on paper
  beyond_weight num = 0.15

section_viewport.update
  id! object
  line object
  name str
  min point
  max point
  center point # [distance along the line, height] in mm
  scale num|enum(fit)
  layers layer[]
  show_frame bool
  cut_weight num
  beyond_weight num
  fit bool # true recomputes center and scale from the drawing

section_viewport.get
  id! object # a section viewport or a section line
  geometry bool = false # true adds all points

group.create
  ids object[] # default: the selection
  name str

group.ungroup
  ids object[] # default: the selection

group.enter
  id! object

group.exit

group.context

group.members
  id! object
  deep bool = false # all levels

group.move_into
  ids object[] # default: the selection
  group! object

group.move_out
  ids object[] # default: the selection

group.move
  ids object[] # default: the selection
  delta! point

group.rotate
  ids object[] # default: the selection
  center! point
  angle! num # radians, counterclockwise

group.copy
  ids object[] # default: the selection
  delta point

selection.click
  id! object
  add bool # toggle in the selection

symbol.list

symbol.get
  id! ref

symbol.create
  name! str
  units enum(world,paper) = world
  insert_class class
  rotate_on_insert bool
  items obj[] # [{shape, attrs?, class?}]: 2D components in definition coordinates
  solids obj[] # [{kind: extrusion, outline, base?, height, color?} | ...]
  insertion point


symbol.from_objects
  name! str
  ids object[] # default: the selection
  insertion point # default: the center
  replace bool # replace the objects by one instance

symbol.place
  symbol! ref
  at! point
  rotation num # radians
  rotate_to point
  scale num = 1
  scale_y num
  layer layer
  class class

symbol.set
  id object
  ids object[]
  @one_of id,ids
  at point
  rotation num
  scale num
  scale_y num|null # null makes the scale symmetric again
  symbol ref # another definition

symbol.convert_to_group
  ids object[] # default: the selection

symbol.edit
  id! object # an instance

symbol.edit_end
  group object # default: the edit group you are in

resource.list
  type str # plant_style, hardscape_style, landscape_area_style, record_format, data_map, worksheet, symbol, hatch, tile, line_type, texture, ...
  search str # part of the name or type
  thumbnails bool # adds an SVG of each

resource.get
  id! ref
  type str

resource.create
  type! enum(symbol,hatch,tile,line_type,texture,plant_style,hardscape_style,landscape_area_style,wall_style,record_format,material,render_style,text_style,dimension_style,tag_style,notes_db)
  name! str
  units enum(paper,world) # hatch, tile, line_type, symbol
  lines obj[] # hatch: [{angle, spacing, offset?, origin?, dash?, color?, weight?}]
  symbol ref # tile
  width num # tile
  height num # tile
  row_shift num # tile
  angle num # tile
  scale num # tile
  elements obj[] # line_type: [{kind: dash | gap | mark, ...}]
  color color # texture
  size num # texture
  reflectivity num # texture
  transparency num # texture
  image any # texture
  @include symbol.create
  @include plant_style.create
  @include hardscape_style.create
  @include landscape_area_style.create
  @include wall_style.create
  @include record_format.create
  @include material.create
  @include render_style.create
  @include text_style.create
  @include dimension_style.create
  @include tag_style.create
  @include notes_db.create

resource.update
  id! ref
  name str # renames
  @include resource.create

resource.rename
  id! ref
  name! str

resource.duplicate
  id! ref
  name str

resource.delete
  id! ref
  reassign ref

resource.purge_unused
  types str[] # default: all but worksheets and data maps
  dry_run bool

resource.thumbnail
  id! ref
  size int = 64 # px

pattern.segments
  pattern! ref
  ring! point[]
  holes point[][]
  scale num = 100 # model mm per paper mm

resource.list_file
  path str
  library str # a file in the library folder
  @one_of path,library

resource.import_from_file
  path str
  library str
  @one_of path,library
  items obj[]|str[] # [{type, name}] or [name]; default: everything when all is true
  all bool
  on_conflict enum(rename,reuse,replace) = rename
  classes str[] # names
  layers str[] # names

resource.export_to_file
  path str
  library str
  @one_of path,library
  ids ref[] # default: all
  overwrite bool

library.list

library.favorites

library.favorite_add
  library! str
  type! str
  name! str
  path str

library.favorite_remove
  library! str
  type! str
  name! str

site.create
  source_ids! id[]
  layer layer # design layer of the site model; default: the layer of the first source
  settings obj

site.update
  id! object
  settings obj
  add_source_ids object[] # 3D loci and 3D polylines that move into the site model
  remove_points int[] # indexes in the source points
  remove_lines int[] # indexes in the source lines
  replace obj[] # [{point: index | line: index, with: id}]: the locus3d or polyline3d takes the place of that source

site.texture
  id! object
  look enum(color,texture,elevation)
  material ref|null # a material whose texture is used; null: the material of the class or of the object slot
  tile_size num # mm, 10 to 1e8
  offset point
  rotation num # degrees, counterclockwise
  bands int # 1 to 64 elevation bands
  ramp color[] # 1 to 16 colors from low to high ground
  range num[]|null # [low, high] mm elevation of the ramp
  bands_at_contours bool

bed.add
  boundary vertex[] # arcs allowed
  from object|object[] # closed shapes: a closed polyline, closed 3D polyline, rectangle, circle, landscape area without holes or hardscape
  @one_of boundary,from
  texture ref|null # a material; omit or null: the material of the slot or of the class
  tile_size num # mm, 10 to 1e8
  offset point
  rotation num # degrees
  own_mapping bool # true gives the bed its own mapping; false uses the mapping of the site model
  keep_source bool
  layer layer
  class class

bed.set
  id! object
  texture ref|null # null for by class
  tile_size num
  offset point
  rotation num
  own_mapping bool # false: use the mapping of the site model
  boundary vertex[]

bed.get
  id! object

bed.list
  site object # only the beds that act on this site model

site.elevation
  id! object
  x! num
  y! num
  surface enum(existing,proposed) = proposed

site.cut_fill
  id! object

site.drape
  id! object
  points! point[]
  closed bool

site.refresh
  id! object

site.contours
  id! object
  surface enum(existing,proposed) = proposed
  points bool = true

survey.import_points
  text str
  path str
  @one_of text,path
  columns str[] # the column order, default ["P","N","E","Z","Code"]
  delimiter enum(auto,comma,semicolon,tab,pipe,space) = auto
  skip_lines int
  unit enum(m,mm,cm,ft,inch) = m # unit of N and E
  z_unit enum(m,mm,cm,ft,inch)
  crs str # EPSG code of a new georeference
  shift bool = true
  layer layer
  class class

tree.import_survey
  text str
  path str
  @one_of text,path
  delimiter enum(auto,comma,semicolon,tab,pipe,space)
  unit enum(m,mm,cm,ft,inch) = m # N and E
  dbh_unit enum(m,mm,cm,ft,inch) = cm
  size_unit enum(m,mm,cm,ft,inch) = m # height and spread
  crs str
  shift bool = true
  on_terrain bool = true # each tree stands on the ground of the site model that covers it
  layer layer
  class class

site.check
  id! object
  xy_tolerance num = 1 # mm
  z_tolerance num = 10 # mm
  spike_threshold num = 1000 # mm
  max_issues int = 1000

site.simplify
  id! object
  tolerance! num # mm, positive
  dry_run bool

georef.survey_point
  model point # mm
  object object # an object whose position is used
  @one_of model,object
  map! point # [easting, northing] in m
  crs str # default: the CRS of the document
  rotation num
  second obj # {model | object, map}: a second known point sets the rotation

view_rules.get

grid.set
  spacing! num # m, positive; null removes the grid
  labels bool = true # default true
  label_height num # model mm, 0 = automatic
  color color # [r,g,b,a]

phase.set
  ids! object[]
  phase! enum(existing,new,demolished)

phase.display
  enabled bool|null # false turns the rules off; null removes them
  existing obj # {visible?, pen?, fill?, weight_factor?}
  new obj
  demolished obj

phase.report

visualization.set
  name str
  field! str # a property such as Condition, Plant.category, Phase, Area or a record field Soil.Depth
  rules! obj[] # [{criterion: {type: equals, value} | {type: range, min?, max?} | {type: otherwise}, color: [r,g,b,a], label?}]; the first rule that fits wins
  target enum(fill,pen,both) = fill
  kinds str[] # shape kinds; empty = all
  include_missing bool
  enabled bool

visualization.auto
  field! str
  bins int # 2 to 64: ranges of a number, light to dark blue
  kinds str[]
  target enum(fill,pen,both)
  name str

visualization.get

visualization.clear

view.screenshot
  region enum(drawing,window) = drawing
  max_width int # px, 64 to 4096

edit.rotate
  ids object[] # default: the selection
  center! point
  angle! num # radians, counterclockwise
  copy bool

edit.scale
  ids object[]
  center! point
  sx! num
  sy num # default sx; a negative factor mirrors
  copy bool

edit.mirror
  ids object[]
  a! point
  b! point
  copy bool = false

edit.array
  ids object[]
  shape! enum(linear,rectangular,circular)
  retain bool = true
  resize num # each copy this many times larger than the one before
  rotate num # radians; each copy turned this much more than the one before
  count int # linear, circular
  offset point # linear: [dx, dy]
  distance num # linear: with angle
  angle num # linear: radians. rectangular: radians. circular: between copies, radians
  columns int # rectangular
  rows int # rectangular
  column_spacing num # rectangular
  row_spacing num # rectangular
  center point # circular
  rotate_objects bool = true # circular

edit.offset
  id! object
  distance num
  side enum(left,right,outside,inside)
  through point # the copy passes through the point
  @one_of distance,through
  copy bool = true

edit.fillet
  radius! num
  mode enum(trim,split,standard) = trim
  a object # between two objects
  b object
  pick_a point
  pick_b point
  id object # inside one shape
  corner int
  corners int[]
  all bool

edit.chamfer
  mode enum(trim,split,standard) = trim
  a object # between two objects
  b object
  pick_a point
  pick_b point
  id object # inside one shape
  corner int
  corners int[]
  all bool
  distance num # from the corner along the first object
  distance2 num # from the corner along the second object (default distance)
  angle num # radians between the first object and the chamfer
  length num # the length of the chamfer line

edit.trim
  id! object
  pick! point
  cutters object[] # default: all other objects of the layer

edit.extend
  id! object
  pick! point
  boundaries object[] # default: all other objects of the layer

edit.split
  id! object
  at point[] # points on the object; it is cut there
  line obj # {a, b}: cut a closed shape along the infinite line through a and b
  @one_of at,line

surface.add
  ids! object[] # 2 or more
  keep bool = false

surface.subtract
  base! object
  cutters! object[]
  keep_cutters bool = false

surface.clip
  ids! object[] # 2 or more
  keep_cutter bool = true

surface.intersect
  ids! object[] # 2 or more
  keep bool = false

surface.compose
  ids! object[] # 2 or more
  keep bool = false

objects.query
  criteria str # the criteria language of worksheet database blocks: Kind = "plant" AND Area > 20
  columns str[] # expressions, e.g. "Area", "Style"; their values are returned per object
  layer layer
  class class
  kind str
  limit int = 1000
  select bool # true selects the matches, except objects of grey layers and classes

snap.get

snap.set
  set map(bool) # {category: bool}
  angle_step_deg num # above 0, at most 180
  relative_angles bool
  distance_step num # mm; 0 uses the grid step
  other_layers bool
  grey bool

snap.query
  point! point
  tolerance num = 10 # mm
  grid_step num = 100 # mm
  anchor point # the last clicked point, for angle, distance, perpendicular and tangent
  prior_direction num # radians
  exclude object[]
  set map(bool) # for this call only

snap.smart
  action! enum(acquire_point,acquire_edge,acquire_at,clear)
  point point
  a point
  b point
  tolerance num
  edges bool # acquire_at: take the edge nearest to the point

visibility.get

visibility.set
  mode! enum(visible,grey,invisible)
  layers layer[] # [ids]
  classes class[] # [ids]

sheet.list

sheet.create
  name! str
  paper str|obj = a3 # a0 a1 a2 a3 a4 letter legal tabloid arch_c arch_d arch_e, or {width_mm, height_mm}
  width_mm num # with height_mm: a paper size of your own
  height_mm num
  orientation enum(landscape,portrait)
  number str
  title str

sheet.set
  layer! sheet
  paper str|obj
  orientation enum(landscape,portrait)
  number str
  title str
  height_mm num # with width_mm: a paper size of your own
  width_mm num

sheet.render
  sheet! str # a sheet layer: ID or name (with compare it may be a sheet of a version)
  format enum(svg,png) = svg
  dpi num = 100 # png
  compare obj # {from, to}: versions or "current"

project.get

project.set
  name str
  client str
  address str
  drawn_by str
  date str
  revision str

project.new_guid

viewport.create
  sheet! sheet
  layers layer[] # design layers; default all
  min point # the frame on the sheet, mm
  max point
  at point
  size point
  scale num|enum(fit) = fit # a denominator such as 100, or "fit"
  center point # model point at the middle of the frame
  view str = plan # plan or a 3D view from view.list
  mode enum(shaded,wireframe,hidden_line,dashed_hidden_line) # 3D only
  render_style ref # a render style of the document; it replaces mode
  dpi num = 150 # 3D only
  crop obj # {type: rect, min, max} | {type: polygon, points} in paper mm
  show_frame bool
  name str

viewport.get
  id! object

viewport.list
  sheet sheet

viewport.update
  id! object
  name str
  min point
  max point
  center point
  scale num|enum(fit)
  layers layer[]
  view str
  mode enum(shaded,wireframe,hidden_line,dashed_hidden_line)
  render_style ref|null # null removes it
  dpi num
  crop obj|null # null removes it
  show_frame bool
  fit bool # true recomputes center and scale

viewport.set_override
  id! object
  layers map(bool|null) # {layer: true | false | null}; keys are layer IDs or names
  classes map(obj|null) # {class: {visible?, pen?, fill?, line_weight?} | null}; keys are class IDs or names
  reset bool

viewport.annotate
  id! object
  add obj[] # [{kind: line, a, b} | {kind: text, at, text, height} | {kind: dimension, a, b, offset, text?} | {kind: shape, shape}]
  remove int[] # annotation ids
  clear bool

title_block.create
  sheet! sheet
  margin num = 10 # mm
  grid_columns int # 0 to 26
  grid_rows int # 0 to 99
  layout obj # {width, height, cells: [{x, y, w, h, label, text, size}]} in mm from the lower left corner of the block; omit for the standard layout
  fields map(str) # {name: text}, for {field.name}

title_block.update
  id! object
  margin num
  grid_columns int
  grid_rows int
  layout obj|null # null for the standard layout
  fields map(str)

title_block.resolve
  id! object

saved_view.list

saved_view.save
  name! str
  zoom num # px per mm
  pan point # model point at the middle

saved_view.restore
  name! str

saved_view.delete
  name! str

text_style.list

text_style.get
  id! ref

text_style.create
  name! str
  font str
  size_mm num # paper
  bold bool
  italic bool
  color color|null
  justify enum(left,center,right)
  line_spacing num

text_style.update
  id! ref
  @include text_style.create

text_style.delete
  id! ref
  reassign any

dimension_style.list

dimension_style.get
  id! ref

dimension_style.create
  name! str
  font str = sans-serif
  text_size_mm num = 2.5 # paper
  unit enum(mm,cm,m,in,ft,ft_in) = mm
  precision int = 0 # decimals
  show_unit bool = false
  angle_precision int = 1
  terminator enum(arrow,open_arrow,tick,dot,none) = arrow
  terminator_size_mm num = 2.5
  ext_gap_mm num = 1 # gap between the object and the extension line
  ext_overshoot_mm num = 2 # extension line past the dimension line
  baseline_spacing_mm num = 7
  text_gap_mm num = 0.8
  prefix str
  suffix str
  tolerance obj # {plus, minus} in the unit of the style
  color color
  round_to num # round values to a multiple of this many real mm

dimension_style.update
  id! ref
  @include dimension_style.create

dimension_style.delete
  id! ref
  reassign any

tag_style.list

tag_style.get
  id! ref

tag_style.create
  name! str
  lines str[] # text with fields in braces: worksheet expressions over the properties of the tagged object
  font str
  size_mm num
  color color|null
  frame enum(none,box,underline,circle)
  leader bool
  terminator enum(arrow,open_arrow,tick,dot,none)
  pad_mm num
  preset enum(area) # the Area tag style

tag_style.update
  id! ref
  @include tag_style.create

tag_style.delete
  id! ref
  reassign any

notes_db.list

notes_db.get
  id! ref

notes_db.create
  name! str
  notes! obj[]|map(str) # [{key, text}] or {key: text}

notes_db.update
  id! ref
  @include notes_db.create

notes_db.delete
  id! ref
  reassign any

dimension.add
  mode! str # horizontal, vertical, aligned, chain_*, baseline_*, ordinate_*, angular, radial, diameter, arc_length
  points point[]
  anchors obj[] # one entry per point: null or {object, vertex: i} | {object, edge: i, t?} | {object, center: true}
  object object # radial, diameter, arc_length: a circle or an arc
  angle num # radians
  offset num # paper mm
  style ref
  text str
  detached bool
  layer layer
  class class
  viewport object

callout.add
  at! point
  leaders obj[] # [{points, object?, vertex?, edge?, center?, t?}]
  tip point # one leader
  object object # one leader: follows the object
  vertex int # one leader
  edge int # one leader
  t num # one leader: the place along the edge
  center bool # one leader
  text str
  note str
  notes_db ref
  style ref
  shoulder_mm num
  terminator enum(arrow,open_arrow,tick,dot,none)
  detached bool
  layer layer
  class class
  viewport object

data_tag.add
  style! ref # a tag style
  object! object
  offset point = [8, 8] # paper mm from the object
  layer layer
  viewport object

data_tag.tag_all
  style! ref
  criteria str
  layer layer
  offset point

legend.add
  at! point # top left
  title str
  criteria str
  group_by str = Style # an expression
  show_count bool
  swatch_mm num
  style ref
  layer layer
  class class
  viewport object

north_arrow.add
  at! point
  size_mm num
  rotation_deg num
  use_georef bool
  arrow_style enum(arrow,compass,simple)
  layer layer
  class class
  viewport object

scale_bar.add
  at! point
  unit enum(m,mm,ft)
  segment num = 5 # length of one segment in the unit
  segments int
  height_mm num
  scale num # denominator, instead of the one of the layer
  style ref
  layer layer
  class class
  viewport object

revision_cloud.add
  points! point[]
  bump_mm num
  revision! str
  date str
  description str
  author str
  mark bool
  layer layer
  class class
  viewport object

annotation.text
  at! point
  text! str
  style ref
  layer layer
  class class
  viewport object

annotation.update
  id! object
  # The fields of the annotation, as annotation.list returns them in items[].annotation. Which
  # ones apply depends on the type of the annotation.
  at point
  text str
  style ref
  offset num|point
  mode str
  points point[]
  anchors obj[]
  object object
  angle num
  detached bool
  leaders obj[]
  tip point
  note str
  notes_db ref
  shoulder_mm num
  terminator enum(arrow,open_arrow,tick,dot,none)
  title str
  criteria str
  group_by str
  show_count bool
  swatch_mm num
  size_mm num
  rotation_deg num
  use_georef bool
  arrow_style enum(arrow,compass,simple)
  unit enum(m,mm,ft)
  segment num
  segments int
  height_mm num
  scale num|null
  bump_mm num
  revision str
  date str
  description str
  author str
  mark bool

annotation.list
  layer layer
  type enum(dimension,text,callout,data_tag,legend,north_arrow,scale_bar,revision_cloud)

annotation.evaluate
  id! object

annotation.apply_text_style
  ids! object[]
  style! ref

revision.list

version.create
  name! str
  note str
  author str
  now_ms int

version.list
  current bool = true # false: leave out current_is_newest, which costs a pass over the drawing; default true

version.get
  id! int|str
  document bool

version.restore
  id! int|str
  now_ms int

version.delete
  id int|str
  ids (int|str)[]
  @one_of id,ids

version.diff
  from! str # a version ID or "current"
  to! str # a version ID or "current"
  kinds enum(objects,layers,classes,resources,document)[] # default all
  limit int = 100 # at most 1000

version.repair
  confirm! bool

issue.list
  status str|str[] # open, in_progress, resolved, closed
  open_only bool
  assignee str
  label str
  object object # the issue is about this object
  text str # in the title or the description
  origin enum(user,agent,script,remote)
  sort enum(created,oldest,modified,status)
  limit int = 200 # at most 1000
  offset int = 0

issue.get
  id! str
  snapshots bool # true: add snapshot_base64 to each viewpoint that has a picture

issue.create
  title! str
  description str
  type str = issue
  status str = open
  priority str = normal
  assignee str
  labels str[]
  due str # YYYY-MM-DD
  objects object[] # they must exist
  point num[] # [x, y, z] in mm
  author str
  now_ms int
  viewpoint obj # {name?, view?, selection?, capture_visibility?, snapshot?, snapshot_base64?}
  capture bool # a viewpoint of the current view
  snapshot bool|obj # true or render.png params

issue.update
  id! str
  title str
  description str
  type str
  status str
  priority str
  assignee str|null
  labels str[]
  add_labels str[]
  remove_labels str[]
  due str|null
  objects object[]
  add_objects object[]
  remove_objects object[]
  point num[]|null
  remove_viewpoints str[]
  confirm bool
  author str
  now_ms int

issue.add_viewpoint
  id! str
  name str
  view obj # {kind: plan, center, zoom} or {kind: camera, target, yaw, pitch, distance, perspective?}
  selection object[] # default: the objects of the issue
  capture_visibility bool = true
  snapshot bool|obj
  snapshot_base64 str
  now_ms int

issue.comment
  id! str
  text! str
  viewpoint str
  author str
  now_ms int

issue.resolve
  id! str
  status enum(resolved,closed)
  comment str
  author str
  now_ms int

issue.delete
  id str
  ids str[]
  @one_of id,ids
  confirm bool
  author str # who deletes; default the collaboration name, else the origin
  purge_snapshots bool # the pictures that no other issue uses go too (refused in a shared session)

issue.import_bcf
  path str
  base64 str
  @one_of path,base64
  on_conflict enum(ask,skip,replace,keep_both) = ask
  skip_items bool
  dry_run bool
  author str
  now_ms int

issue.export_bcf
  path str # a file that ends in .bcfzip; omit to get the file as base64
  version enum(3.0,2.1) = 3.0
  ids str[]
  status str|str[]
  open_only bool
  assignee str
  label str
  object object
  overwrite bool

dxf.compare
  base! str # the old file
  new! str # the new file
  options obj # {tolerance? (units of the drawing), layers? (only entities on these DXF layers), limit? (default 200, at most 1000), ...import options such as hatches}
  render obj # {format: png|svg|pdf, revision_clouds?, width?, height?, palette?, unchanged?, dark?, ghosts?, previous?, view?}

collab.status

collab.connect
  url str
  doc str
  token str
  replace bool
  user str
  name str
  store str
  discard bool
  now_ms int

collab.disconnect

collab.sync
  now_ms int

collab.presence
  cursor point
  selection object[]
  viewport obj # {center, scale, size}
  clear bool
  now_ms int

collab.undo
  now_ms int

collab.redo
  now_ms int

events.subscribe
  kinds enum(objects,layers,classes,resources,document,issues)[]

events.unsubscribe

events.poll
  since_revision! int
  kinds enum(objects,layers,classes,resources,document,issues)[]

feedback.submit
  kind! enum(bug,feature_request,question)
  title! str
  body str
  wanted str
  tried str
  method str
  error str
  severity enum(low,medium,high)
  attach bool|obj # {excerpt?, screenshot?, calls?, log?, objects?, selection?, view?}
  client obj # added by the host; the program name and its version

feedback.list
  status str
  limit int

feedback.get
  id! str

feedback.withdraw
  id! str

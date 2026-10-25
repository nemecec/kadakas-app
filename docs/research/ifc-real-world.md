# Real-world IFC: what exporters write and importers expect (IOP-29)

Scope: public sources only. We did not download or run Vectorworks, Revit, ArchiCAD, Allplan, Civil 3D, Tekla or Bentley products. Every claim has a source URL. A claim marked unverified rests on one forum post or on a search excerpt. We did not open that source in full.

Fetch notes. The buildingSMART spec pages (standards.buildingsmart.org, ifc43-docs) returned 403 to our fetch. We read the same texts from the public source repository https://github.com/buildingSMART/IFC4.3.x-development (branch ifc4.3-main, path docs/schemas). Graphisoft help pages and some forums returned redirects or 403. We saw only search excerpts of them. We mark those items unverified.

We downloaded four public sample files into a scratch folder and counted their entities. We did not run Kadakas on them. So every statement of the form "Kadakas reads this" below comes from reading the code, not from a test run.

## 1. IFC versions and views by product

| Claim | Source |
|---|---|
| Vectorworks 2022 help: IFC 2x2, 2x3 and 4. Certified export: IFC2x3 Coordination View 2.0 Architecture and IFC4 Reference View 1.2. Certified import: any IFC2x3 CV 2.0 model. | https://app-help.vectorworks.net/2022/eng/VW2022_Guide/Export/..\IFC\IFC_format_interoperability.htm |
| Vectorworks 2025 and 2026 help: export of IFC 2x3, 4 and 4x3. Files .ifc, .ifczip and .ifcxml. | https://app-help.vectorworks.net/2026/eng/VW2026_Guide/IFC/Exporting_IFC_projects.htm |
| Vectorworks export views: CV 2.0 Architecture, Extended Vectorworks Model View (adds IfcBuildingElementProxy geometry), IES VE, COBie, Simplified Geometry, Reference View (IFC4 only). | same page |
| Vectorworks import reads .ifc, .ifczip and .ifcxml. Georeferencing from the file is offered. Units follow the Vectorworks file. | https://app-help.vectorworks.net/2024/eng/VW2024_Guide/Workgroup/..\IFC\Importing_IFC_files.htm |
| Vectorworks import: only IfcSpace becomes a native Space. All other objects become a plug-in object "IFC Entity". | IFC_format_interoperability page above |
| Revit offers IFC2x3 Coordination View 2.0 (default), IFC4 Reference View and IFC4 Design Transfer View. File types IFC, IFC XML, zipped IFC, zipped IFC XML. | https://help.autodesk.com/cloudhelp/2025/ENU/Revit-DocumentPresent/files/GUID-E029E3AD-1639-4446-A935-C9796BC34C95.htm |
| Revit IFC4 Design Transfer View sometimes writes IfcAdvancedBrep and gives smaller files. Reference View is not modified by the receiver. | same page |
| Tekla offers Reference view and Design transfer view for IFC4 and IFC4.3. Certified: IFC2x3 CV 2.0, IFC4 Reference view, IFC4.3 Bridge view (2025 page). | https://support.tekla.com/doc/tekla-structures/2026/int_exporting_into_ifc (search excerpt, unverified) |
| Allplan imports .ifc, .ifcXML and .ifczip. It maps IfcSite to a DTM and unknown classes to IfcBuildingElementProxy. | https://help.allplan.com/Allplan/2023-0/1033/Allplan/320465.htm |
| Civil 3D exports TIN surfaces to IFC only after conversion to 3D solids. Alignments are the exception. | https://help.autodesk.com/cloudhelp/2025/ENU/Civil3D-UserGuide/files/GUID-C5C9DEEE-2C46-4094-B350-05829C1ED5DC.htm (search excerpt, unverified) |
| An IFC 4x3 extension for Civil 3D 2022 exports IFC 4, 4x3 RC4 and 4x3. | https://forums.autodesk.com/t5/civil-3d-forum/ifc-export-and-ifcinfra-export/m-p/13976132 (search excerpt, unverified) |
| ArchiCAD IFC4 export creates IfcMapConversion and IfcProjectedCRS. Datum and projection fields are required. | https://helpcenter.graphisoft.com/user-guide/89325 (search excerpt, unverified) |
| Bentley Building Designer writes NURBS in IFC4 as a technology preview. | https://github.com/buildingsmart-community/Community-Sample-Test-Files (folder IFC 4.0.2.1 (IFC 4)/NURBS/Bentley Building Designer, Application.md) |
| 12d Model writes IFC 4x3 RC1 TIN files with IfcTriangulatedIrregularNetwork. | https://github.com/buildingsmart-community/Community-Sample-Test-Files (dev archive, Tin_1 and Georeferencing_Tin_1 readme files) |
| A SketchUp exporter (BIM Tools "IFC manager") wrote the buildingSMART landscaping scene in IFC2X3, IFC4 and IFC4X3_ADD2. | https://github.com/buildingSMART/Certification-datasets (header of Simple-Scene/Infra-Landscaping.ifc) |

Lesson: the same product writes several versions. IFC2x3 stays the common denominator. IFC4 Reference View is the modern exchange target. Expect IFC4x3 mostly from infrastructure tools.

## 2. Landscape objects: entities and representations

### 2.1 What the exporters write

| Object | Entity and representation | Source |
|---|---|---|
| Vectorworks Site Model | IfcSite | Vectorworks interoperability page (section 1) |
| Vectorworks Existing Tree, Plant, Landscape Area, Roadway | IfcBuildingElementProxy | same |
| Vectorworks Hardscape | IfcSlab | same |
| Vectorworks Landscape Wall | IfcWall | same |
| Vectorworks Grade | IfcAnnotation | same |
| Vectorworks Railing/Fence, Guardrail | IfcRailing | same |
| Vectorworks Parking Area, Parking Spaces | IfcSpace | same |
| Vectorworks Massing Model | IfcBuilding | same |
| Vectorworks has an option "Export Site Model" and one "Export Solids Operations as BREPs". | | Exporting_IFC_projects page (section 1) |
| Revit 2018 turned Topography into IfcSite for IFC2x3. For IFC4 it was not exported at all (Feb 2019 report). | | https://forums.buildingsmart.org/t/topography-with-ifcgeographicelement/913 |
| Revit 2019 exports Topography as IfcGeographicElement when the parameter IfcExportAs is set. | | same thread |
| Revit merges all topography pieces into one IfcSite. Workaround: map to IfcGeographicElement (IFC4) or IfcBuildingElementProxy with user-defined type SITEMODEL or TERRAIN_SURFACE. Unverified, one thread. | | search excerpt of a buildingSMART forum thread, see https://forums.buildingsmart.org/t/questions-about-ifc-site-in-ifc-4-0/4060 |
| Revit IfcSite composition type is fixed to ELEMENT. Revit does not nest spatial structures. | | https://wiki.osarch.org/revit-setup-for-openbim-revit-ifc-spatial-hierarchy/ (search excerpt, unverified) |
| ArchiCAD 23 and later write elements classified as IFC Site Geometry as solid bodies. Several elements classified IfcSite become one entity in the file. | | https://help.graphisoft.com/AC/24/INT/_AC24_Help/115_IFC/115_IFC-37.htm |
| ArchiCAD users report a mesh classed as Site Geometry not exported. Workaround from a reply: class it as proxy. Unverified, one thread. | | https://community.graphisoft.com/t5/Collaboration-with-other/export-site-geometry-IFC/td-p/262852 |
| SketchUp exporter: terrain and vegetation are IfcGeographicElement with IfcTriangulatedFaceSet, grouped in IfcSite with CompositionType PARTIAL, sites aggregated under one COMPLEX site. | | sample file Infra-Landscaping.ifc (CC BY 4.0) |
| 12d Model: terrain is IfcSolidStratum (IFC4x3 RC1) with IfcTriangulatedIrregularNetwork. Triangles carry flags -2, -1 and 7 in the sample. | | Tin_1 readme (community repo) |

### 2.2 What the 4x3 specification says

- IfcGeographicElement covers "trees or terrain". Predefined types: TERRAIN, SOIL_BORING_POINT (deprecated), VEGETATION, USERDEFINED, NOTDEFINED. Source: https://github.com/buildingSMART/IFC4.3.x-development/blob/ifc4.3-main/docs/schemas/core/IfcProductExtension/Entities/IfcGeographicElement.md and Types/IfcGeographicElementTypeEnum.md.
- IfcGeographicElement is new in IFC4. So IFC2x3 has no IfcGeographicElement and no TERRAIN. Source: same entity page (history note).
- A feature catalog goes in IfcClassificationReference with Catalog, Identity and ElementName. Source: same entity page.
- IfcCivilElement is deprecated in IFC4.3. Specific subtypes replace it. Source: .../IfcCivilElement.md.
- IfcBuildingElementProxy "no longer" fits spatial placeholders (the page says to use IfcVirtualElement instead). The page advises IfcBuiltElement for arbitrary elements. Source: .../IfcBuildingElementProxy.md.
- IfcPavement (types FLEXIBLE, RIGID) and IfcCourse are new infrastructure classes of IFC4.3. Source: .../IfcPavement.md, .../IfcCourse.md.
- Psets of the 4x3 repository for these classes: Pset_SiteCommon, Qto_SiteBaseQuantities, Pset_SlabCommon, Qto_SlabBaseQuantities, Pset_PavementCommon, Pset_CourseCommon, Qto_PavementBaseQuantities, Qto_CourseBaseQuantities. Source: file list of the repository (docs/schemas).
- No Pset exists in that list for IfcGeographicElement. So plant data goes in custom psets or classification. This reads from the file list only. Treat as unverified for a full search.
- IfcAnnotation carries 2D plan content. Source: .../IfcAnnotation.md (class exists in the tree).

### 2.3 Representation types seen in the wild

| Type | Seen from | Source |
|---|---|---|
| SweptSolid, IfcExtrudedAreaSolid | Revit ("the safe option"), ArchiCAD extruded mode | https://wiki.osarch.org/revit-setup-for-openbim-revit-and-ifc-geometry/ , ArchiCAD page above |
| IfcFacetedBrep (closed shell with polyloops) | IFC2x3 exports, SketchUp IFC2x3 sample (59 breps, 6658 faces) | sample Infra-Landscaping.ifc IFC2X3 |
| IfcTriangulatedFaceSet | IFC4 Reference View, SketchUp IFC4 sample (42 sets) | sample Infra-Landscaping.ifc IFC4 |
| IfcAdvancedBrep with B-spline curves and surfaces | Revit Design Transfer View, BricsCAD BIM 26.1 (IFC4X3_ADD2), Bentley | Revit page above, https://github.com/IfcOpenShell/IfcOpenShell/issues/7728 , Bentley folder |
| IfcTriangulatedIrregularNetwork | 12d Model | Tin_1 readme |
| IfcBooleanResult, clipping | ArchiCAD booleans, Revit | https://github.com/IfcOpenShell/IfcOpenShell/issues/2320 (title only, unverified) |
| IfcMappedItem with IfcRepresentationMap | types in all four sample scenes (IfcRelDefinesByType count 6 to 24) | sample files |
| Gross body plus IfcOpeningElement | ArchiCAD default | ArchiCAD page above |
| IfcBoundingBox "Box" representation | Revit option "Export bounding box" | Revit page above |

Further facts:

- ArchiCAD offers extruded, BREP and "BREP with junctions" geometry per category. Extruded mode keeps parameters. BREP mode loses them. Source: ArchiCAD geometry page above.
- ArchiCAD exports slabs with vertical edges in extruded mode. Slanted edges need BREP. Source: same page. For landscape slabs this matters for hardscape with slope.
- Revit swept, revolved and blended forms with curves become heavy meshes. A fillet on a swept profile grew a file from 10 kB to 270 kB. Source: the OSArch Revit geometry page above.
- Revit re-export as IFC4 shrank one object from 16,098 kB to 4,487 kB. Source: same page.
- Revit option "Keep Tessellated Geometry as Triangulation" exists for older Reference View viewers. Source: Revit page above.

## 3. Units, scale and placement

| Claim | Source |
|---|---|
| Length units seen: milli metre in the buildingSMART landscaping scene (IfcSIUnit MILLI METRE, area and volume in SI base). | sample files |
| Allplan imports the length unit of the file and offers a resize factor. | Allplan page above |
| Vectorworks import takes units from the Vectorworks file, not from the IFC file. So a unit mismatch is a known risk. | Vectorworks import page above |
| IfcMapConversion.Scale converts the units of the project to the units of the map CRS. "If omitted, the scale factor 1.0 is assumed." | https://github.com/buildingSMART/IFC4.3.x-development/blob/ifc4.3-main/docs/schemas/resource/IfcRepresentationResource/Entities/IfcMapConversion.md |
| IfcMapConversionScaled (IFC4X3_ADD1) adds FactorX, FactorY and FactorZ for grid scale. | .../IfcMapConversionScaled.md |
| A units mismatch between IfcMapConversion and the project is a reported Revit IFC problem (GitHub issue 290 of the Revit IFC project). Details are not given. Unverified. | https://wiki.osarch.org/revit-setup-for-openbim-revit-ifc-geolocation/ |
| IFC files do not support the US survey foot, so a small shift appears. Unverified, search excerpt. | https://support.esri.com/en-us/knowledge-base/discrepancy-on-the-geolocation-of-an-ifc-file-ifc2x3-or-000027481 |
| The IfcSite placement is relative to the spatial parent or absolute in the world system of the project. | .../IfcSite.md in the repository |
| A large offset on the IfcSite placement on top of the map conversion doubles the shift. Unverified, one forum post. | search excerpt of https://community.osarch.org/discussion/comment/5503/ |
| Revit IFC4 needs patching for correct geolocation. The patch resets IfcSite and IfcBuilding locations to remove double offsets. | https://wiki.osarch.org/revit-setup-for-openbim-revit-ifc-geolocation/ |
| Placement chain in the SketchUp scenes: one IfcLocalPlacement per object, site placements relative to the project, objects relative to their site (8 sites, 119 placements). | sample file Infra-Landscaping.ifc |
| IfcGridPlacement is rare. IfcOpenShell lacked support for it until a fix (issue closed). | https://github.com/IfcOpenShell/IfcOpenShell/issues/1346 |

## 4. Georeferencing

- IFC4 and later: IfcMapConversion plus IfcProjectedCRS on the geometric context. The buildingSMART sample writes `IFCPROJECTEDCRS('EPSG:32760',...)` and `IFCMAPCONVERSION(#11,#18,729011.2258823584,9063960.607644705,0.,1.,0.,0.001)` (Eastings, Northings, height, axis abscissa, ordinate, scale 0.001 because the model is in mm). Source: sample file Infra-Landscaping.ifc IFC4.
- Note the scale 0.001 in that sample. A reader applies the scale to the model units. This is the exact case of a mm model with a metre CRS.
- IFC2x3: no geolocation entities. The convention is the property sets EPset_ProjectedCRS and EPset_MapConversion on the top IfcSite. Properties: Name, Description, GeodeticDatum, VerticalDatum, MapProjection, MapZone, MapUnit and Eastings, Northings, OrthogonalHeight, XAxisAbscissa, XAxisOrdinate, Scale. Source: https://forums.buildingsmart.org/t/geolocation-standards-in-ifc2x3-and-ifc4/2329
- Spelling differs: `EPset_` in the forum thread, `ePSet_` in the user guide. Source: same thread. Importers match the name without case.
- If both exist, the real IfcMapConversion wins. Source: same thread.
- Revit IFC2x3 geolocation needs a Toposurface exported as IfcSite and a text file that defines the psets. Source: Revit geolocation wiki above.
- IfcSite RefLatitude and RefLongitude are integer lists (degrees, minutes, seconds, millionths of seconds). RefElevation is relative to sea level. The spec calls the attributes approximate and not a replacement for precise referencing. Source: IfcSite.md in the repository.
- Revit sets RefLatitude and RefLongitude often wrongly. It cannot set RefElevation on export. Source: Revit geolocation wiki above.
- Vectorworks export: georeferencing from the document or custom. IFC 2x3 exports geolocated coordinates. IFC4 and later also export the coordinate system. Origin choices: Internal, User, Custom, Survey Point, Stake. Source: Vectorworks export page.
- Vectorworks import offers: use the document system, use the IFC system, or set the document system to match. Source: Vectorworks import page.
- Civil 3D, Bentley, Tekla: georeferencing details not found in public pages we did open. Not covered.

## 5. Property sets, quantities, classification

- Vectorworks export attaches Psets by attachments on objects. The help says some default attachments need customizing and objects without them need new ones. Source: Vectorworks interoperability page.
- Vectorworks has "Export Custom Property Sets" and filters Psets and (IFC4 only) quantity sets on import. Source: Vectorworks export and import pages.
- Revit option "Export base quantities" writes Qto sets from geometry. Source: Revit page above.
- Tekla offers Default and Minimum property set exports. Source: Tekla 2020 export page (search excerpt, unverified).
- The SketchUp scene writes a custom IfcPropertySet "SizeSet" with a Height as IfcLengthMeasure(167.) in mm units. Source: sample Infra-Landscaping.ifc.
- Classification: the sample uses IfcClassification "Molio" CCI Construction. A reference points to class L-TRA "Tree". A bSDD agri classification gives "apple". Source: sample file. The identifiers are buildingSMART bSDD URIs.
- Common systems in the Bonsai classification library: Uniclass (NBS Create), OmniClass (Omniclass_OCCS.ifc), CCI, MasterFormat, STABU and others. Source: https://github.com/IfcOpenShell/IfcOpenShell (src/bonsai/bonsai/bim/data/classifications).
- IfcOwnerHistory is mandatory in IFC2x3 only. The IFC4 SketchUp sample still writes one. Importers need to accept both.

## 6. Spatial tree, types, materials, styles, layers

- Containment: IfcRelContainedInSpatialStructure holds products. It does not hold other spatial elements. Aggregation builds the spatial tree. Source: https://github.com/buildingSMART/IFC4.3.x-development/blob/ifc4.3-main/docs/schemas/core/IfcProductExtension/Entities/IfcRelContainedInSpatialStructure.md
- The same page names the retaining wall in terrain as an example for site containment.
- The sample scene aggregates 7 partial sites under one site, under the project. Elements sit in the partial sites by containment. Assemblies (IfcElementAssembly) aggregate parts. Source: sample file.
- Types: IfcRelDefinesByType with IfcGeographicElementType, IfcBuildingElementProxyType, IfcMemberType. 24 relations in the IFC4 sample. Source: sample file.
- Materials: IfcMaterial and IfcRelAssociatesMaterial, one material per surface style (11 and 11). Layer sets are not used in the landscape sample. Source: sample file.
- Styles: IfcStyledItem on the tessellated set, IfcSurfaceStyleRendering, IfcColourRGB. Source: sample file. The 2x3 file uses IfcPresentationStyleAssignment.
- Openings: IfcOpeningElement is subtracted from the Body of the voided element via IfcRelVoidsElement. A representation with identifier Reference is not subtracted. Source: .../IfcOpeningElement.md in the repository.
- ArchiCAD exports gross geometry and separate openings by default. Extruded elements with holes fall back to BREP when gross geometry is off. Source: ArchiCAD geometry page.
- Presentation layers: IfcPresentationLayerAssignment. Not seen in the four sample scenes. Vectorworks maps them to classes with prefix "IFC-". Source: Vectorworks import page.
- Stories: Vectorworks maps each IFC story to a story and offers an elevation offset. Source: Vectorworks import page.
- Phasing: Vectorworks maps phases to IFC status values NEW, DEMOLISHED, EXISTING, TEMPORARY. It states a phase is a time sequence and a status a static state. Source: Vectorworks export page.

## 7. File size and performance

- Revit: a 10 MB RVT gave a 140 MB IFC in one report. Unverified, one forum post. Source: https://community.osarch.org/discussion/comment/2548 (search excerpt).
- A Vectorworks IFC4 file of 280 MB crashed Bonsai on two Intel Macs, while BIMcollab Zoom opened it. Source: https://github.com/IfcOpenShell/IfcOpenShell/issues/8115 (open, no repro file).
- A 2 GB IFC from Synchro ran a 16 GB machine out of memory in IfcOpenShell. Source: https://github.com/IfcOpenShell/IfcOpenShell/issues/2025
- The Schependomlaan design model "IFC_Schependomlaan_geo.ifc" is 49 MB. Source: Git LFS pointer in the community repo.
- Our reader caps are 1 GiB and 20 million entities (IOP-20). A 2 GB file exists in the wild, so the cap is a real limit.

## 8. STEP syntax quirks

- `\X2\...\X0\` is UTF-16 and `\X4\...\X0\` is UTF-32. Software reads X2 well. Some readers fail on X4 (Solibri case). IfcOpenShell once wrote `\X4\000000F8\X0\` for "ø" and readers rejected it. Source: https://github.com/IfcOpenShell/IfcOpenShell/issues/699
- Real files use `\X\27` (ISO 8859 hex escape) for the apostrophe. The buildingSMART landscaping sample has "earth\X\27s". Source: sample file Infra-Landscaping.ifc (CC BY 4.0).
- `\S\` (ISO 8859 upper half) and `\P` (code page) are in ISO 10303-21. We found no public sample with them. Unverified in practice.
- Vectorworks has an option "Export Text in Unicode Format". The help says it is more reliable for double-byte characters but the receiver has to support it. Source: Vectorworks export page. So Vectorworks writes non-ASCII text in other ways than X2 in some modes (unverified).
- A Vectorworks IFC4X3 file had lowercase entity names (`ifcactorrole(.architect.,$,$)`), lowercase enums, duplicate GlobalIds and duplicate STEP ids. A maintainer said the file had "invalid behaviour galore". Source: https://github.com/IfcOpenShell/IfcOpenShell/issues/5213 (the file was possibly edited, per the thread).
- Also from that file: a space after the equals sign, as in `#2= ifcactorrole(...)`. Source: same issue.
- Files carry sometimes a byte order mark, comments and complex entities. The IOP-20 reader covers these.
- Entities span lines at no fixed wrap width. Exporters differ. Not found in a public source beyond file inspection.
- FILE_SCHEMA with more than one name is allowed by the STEP standard. Our reader returns all names. Exporters we saw write one name. Unverified in practice.
- ifcZIP: a zip with one .ifc inside. ifcXML and ifcJSON are other encodings. Vectorworks, Revit and Allplan accept or write ifcZIP and ifcXML. Sources: Vectorworks, Revit, Allplan pages above. The buildingSMART formats page returned 403, so ifcJSON status is unverified.

## 9. Known interop problems from users

| Problem | Source |
|---|---|
| Terrain goes into IfcSite for IFC2x3 and disappears or moves for IFC4 in some Revit versions. | https://forums.buildingsmart.org/t/topography-with-ifcgeographicelement/913 |
| FreeCAD errored on IfcGeographicElement export in early 2019. A SketchUp topography lost its geometry after re-export. | same thread |
| Several IfcSite elements merge into one on export. Piecewise terrain is lost. | ArchiCAD and Revit threads above, unverified |
| Allplan treats IfcSite as a DTM. So an IfcSite with a face set becomes terrain there. | Allplan page above |
| Vectorworks turns most imported classes into a generic "IFC Entity" object. | Vectorworks interoperability page |
| Missing opening cut in ArchiCAD roofs on import into IfcOpenShell. | https://github.com/IfcOpenShell/IfcOpenShell/issues/3437 (title only) |
| IfcAdvancedBrep with a revolved surface and omitted Axis in IfcAxis1Placement crashed IfcOpenShell. Fixed. | https://github.com/IfcOpenShell/IfcOpenShell/issues/7728 |
| IfcAdvancedBrep box with a cylindrical hole gave a volume 12.9 percent too large in IfcOpenShell (web build). | https://github.com/IfcOpenShell/IfcOpenShell/issues/9570 (title only) |
| Duplicate types from mirrored Revit elements bloat the file. | Revit geometry wiki above |
| Georeferencing breaks when IfcSite offsets add to the map conversion. | OSArch posts above, unverified |
| Wrong latitude and longitude in Revit files. | Revit geolocation wiki above |

## 10. Public sample files with a clear licence (test corpus)

Licence check. The repository Certification-datasets has a LICENSE text: "licensed under the Creative Commons Attribution 4.0 International License" (CC BY 4.0). Source: https://raw.githubusercontent.com/buildingSMART/Certification-datasets/main/LICENSE . The GitHub API shows NOASSERTION for it, so the LICENSE text is the evidence. The community repository carries a LICENSE of the same kind and its README says contributors publish under CC BY 4.0. It adds "(C) original authors". Source: https://github.com/buildingsmart-community/Community-Sample-Test-Files . Attribution is required. Our docs/attribution.md lists each file.

Usable (CC BY 4.0):

| Name | URL | What it covers | Size |
|---|---|---|---|
| Infra-Landscaping.ifc (IFC4) | https://github.com/buildingSMART/Certification-datasets (IFC 4.0.2.1 (IFC 4 ADD2 TC1)/Simple-Scene) | 8 sites (partial), terrain and vegetation as IfcGeographicElement, tessellated sets, types, 11 materials and styles, Molio and bSDD classification, IfcMapConversion with scale 0.001, `\X\27` strings | 2.3 MB |
| Infra-Landscaping.ifc (IFC4X3_ADD2) | same repo, folder IFC 4.3.2.0 (IFC 4.3 ADD2) | same scene, plus IfcSign and IfcSignType | 2.3 MB |
| Infra-Landscaping.ifc (IFC2X3) | same repo, folder IFC 2.3.0.1 (IFC 2x3 TC1) | same scene as 91 proxies, faceted breps, IfcMember, element assemblies, no geolocation | 1.2 MB |
| Building-Landscaping.ifc (IFC4, 4X3, 2X3) | same repo | small landscaping scene of seven tessellated sets, geographic elements and proxies, georeference | 1.1 MB |
| Infra-Road, Infra-Bridge, Infra-Rail, Infra-Electrical, Infra-Plumbing | same repo | infrastructure scenes in three schema versions, for stress and for skipped-class reports | 0.1 to 1.7 MB each |
| Building-Architecture.ifc (three versions) | same repo | walls, slabs, openings, storeys | 0.14 MB |
| basin-tessellation.ifc, tessellated-item.ifc, tessellation-with-individual-colors.ifc, column-straight-rectangle-tessellation.ifc | same repo, folder ISO Spec - ReferenceView_V1.2 | Reference View tessellation, colours per face | 2.6 to 12 KB |
| wall-with-opening-and-window.ifc | same repo, same folder | IfcOpeningElement, IfcRelVoidsElement, IfcRelFillsElement | 12 KB |
| UT_Tin_1.ifc | https://github.com/buildingsmart-community/Community-Sample-Test-Files (dev archive/IFC 4.3.RC Archive/Tin_1 (RC1)) | 12d Model TIN with holes, IfcTriangulatedIrregularNetwork, IfcSolidStratum (IFC4x3 RC1 name) | 179 KB (Git LFS) |
| UT_Tin_in_MGA_56.ifc | same repo, Georeferencing_Tin_1 (RC1) | TIN in map coordinates, IfcProjectedCRS and IfcMapConversion, datums | 67 KB (Git LFS) |
| UT_GeoRef_1.ifc | same repo, Georeferencing-1 (RC2) | georeferencing scenario for 4x3 RC2 | 4 KB (Git LFS) |
| geographic-referencing.ifc | same repo, IFC 4.0.2.1 (IFC 4)/ISO Spec archive | IFC4 georeferencing example | 3 KB (Git LFS) |
| Bentley Building Designer cylinder files (BsplineSurf.PolyCurve.ifc, Facet.ifc, OnlyBsplines.ifc, Plane.Cyllinder.PolyCurve.ifc) | same repo, IFC 4.0.2.1 (IFC 4)/NURBS/Bentley Building Designer/Cylinder | IfcAdvancedBrep with B-spline surfaces and a facet twin of the same shape | small (Git LFS) |

Notes:

- Files marked Git LFS need `git lfs` or the media URL. A raw download returns a pointer of about 130 bytes.
- Five files of the community repo are listed with the licence of the repo. Each folder owner stays the author. Verify the folder readme before copying into the repo.
- The ISO Spec archive also has ifcXML, ifcJSON and Turtle twins of the same models. Good for an ifcXML test (xml: wall-with-opening-and-window.xml, surface-model.xml).

NOT usable (licence unclear or restricted):

| Name | Reason |
|---|---|
| Schependomlaan dataset (all files, 49 MB design model, ifcZIP subcontractor models) | README: "permission to use the data for scientific and academic purposes". That is not an open licence. |
| Duplex Apartment files in the community repo | No licence statement of its own found in the folder. Treat as unclear. |
| Files attached to IfcOpenShell issues (Vectorworks, BricsCAD, ArchiCAD models) | Users posted them for debugging. No licence. Do not copy. |
| Any file from a vendor sample library (Revit, ArchiCAD, Vectorworks, Allplan, Tekla) | Not opened. Not allowed in this research. |
| IfcOpenShell repository test files | The code is LGPL-3.0 (GitHub API). Individual test files carry no stated data licence. Generate files at test time, as tests/behavior/test_ifc_import.py does. |

## 11. What Kadakas supports today (reading docs and code)

Read: docs/interop.md, docs/feature-spec.md rows IOP-02 to IOP-07, IOP-20, IOP-26, docs/exact-solids.md, crates/kadakas-ifc, crates/kadakas-step/src/part21.rs, tests/fixtures/ifc.

Supported:

- Import geometry: extruded area solids with many profile kinds. Triangulated and polygonal face sets. Faceted breps. Shell and face based surface models. Mapped items. Boolean clipping with a half space. Bounding boxes. IfcTriangulatedIrregularNetwork. Polylines and indexed polycurves.
- Import structure: storeys become layers and presentation layers become classes. Psets and Qtos become records. Types and colours are read. The georeference comes from IfcMapConversion or ePSet_MapConversion. Unit chains work.
- Export: IFC2X3, IFC4, IFC4X3_ADD2 with terrain, slabs, areas, plants, walls, georeference, layers, stable GlobalIds.
- String decoding: `\X2\` and `\X4\` (crates/kadakas-step/src/part21.rs line 197). Other escapes "stay as they are".
- Reader: streaming, caps, comments, complex entities, broken statements.

Not supported or unknown:

- ifcZIP, ifcXML, ifcJSON: no code found (only a mention in docs/research/vectorworks-public-features.md).
- IfcOpenings not cut (documented in IOP-03).
- No import of IfcRevolvedAreaSolid, IfcSweptDiskSolid, IfcCsgSolid, IfcBlock, tapered or fixed-reference sweeps. The importer knows IfcAdvancedFace. IfcAdvancedBrep as a whole is reported as unsupported with a box stand-in (import/geom.rs test line 1757).
- IfcGridPlacement: no mention in the code.
- IfcMapConversionScaled is read as IfcMapConversion. The factors are ignored (import/model.rs line 456).
- Fixtures are hand-written and small (1 to 2 KB each). No fixture is a real exporter file.

## 12. Ranked gaps and recommended tests

1. Escapes `\X\`, `\S\` and `\P` are not decoded. A real CC BY file uses `\X\27`. Test: import Infra-Landscaping.ifc (IFC4) and assert the name of the underground element contains an apostrophe ("earth's") and no backslash. Add a unit test in part21.rs for `\X\41`, `\S\d` and `\P` forms.
2. No ifcZIP reading or writing. Vectorworks, Revit and Allplan use it. Test: zip tests/fixtures/ifc/extruded_rect.ifc and assert the import report equals the plain file. Test a zip with two entries and a zip bomb with a size cap. Test an export to .ifczip that Python `zipfile` and IfcOpenShell open.
3. Real-world corpus is missing. Test: a fetch-at-run-time test (like the DXF one in IOP-15) over the CC BY files of section 10. Assert no panic. Assert the counts of terrain, plants and sites (8 sites, 76 geographic elements in the IFC4 scene). Assert the georeference (EPSG:32760, scale 0.001 applied) and a time limit.
4. Nested IfcSite trees (COMPLEX site, PARTIAL sites) and IfcElementAssembly. Landscape files from SketchUp-like tools use them. Test: import Infra-Landscaping.ifc. Assert every IfcGeographicElement ends on a layer. Assert terrain elements become site models (11 expected). Assert vegetation (9 with USERDEFINED and lower-case ObjectType) becomes plants or areas.
5. Openings are not cut. ArchiCAD exports gross geometry plus IfcOpeningElement. Test: import wall-with-opening-and-window.ifc and assert the mesh volume equals gross volume minus the opening. This needs the feature first. Until then assert the report note and a Reference-representation case that must not be cut.
6. Advanced BREP with B-spline surfaces (Revit Design Transfer, BricsCAD, Bentley) falls back to a box. Test: import the Bentley cylinder files and compare the stand-in box with the bounding box of the Facet.ifc twin. Later test: the tessellated volume of the B-spline file is within 1 percent of the facet twin.
7. Missing swept solids: IfcRevolvedAreaSolid, IfcSweptDiskSolid (pipes, irrigation), IfcCsgSolid. Test: generate files with IfcOpenShell at test time and compare volume to the analytic value.
8. IfcTriangulatedIrregularNetwork flags and holes. 12d Model writes flags -2, -1 and 7. Test: import UT_Tin_1.ifc and assert the hole triangles are skipped according to the flags and the vertex count is stable.
9. Map conversion edge cases: scale 0.001, IfcMapConversionScaled factors, unit of CRS in feet, double offset from the IfcSite placement. Test: UT_Tin_in_MGA_56.ifc and a fixture with an IfcSite offset plus a map conversion. Assert the site point lands where the map conversion says. Also test ePSet name spelling in three cases (ePSet_, EPset_, epset_).
10. Large-file behaviour. Test: concatenate sample scenes to about 200 MB and assert time, memory and the cap messages. Also a file over 1 GiB gives a clear error.
11. IfcGridPlacement and IfcLinearPlacement. Test: fixture with IfcGridPlacement and assert a report note, not a crash.
12. Lowercase entity names and a space after `=` (the Vectorworks case in issue 5213). Test: fixture with `#2= ifcactorrole(.architect.,$,$);` and assert the parse keeps going. Duplicate GlobalIds and duplicate step ids give a report note.
13. Export check for Allplan-style readers: IfcSite with terrain. Allplan reads IfcSite as DTM, so also write a face set on the IfcSite when asked. Test: export option and an IfcOpenShell read of both.

## 13. Summary

Top 5 findings:

1. Landscape objects have no single convention. Vectorworks writes plants and areas as IfcBuildingElementProxy, hardscape as IfcSlab and terrain as IfcSite. SketchUp-style tools and the buildingSMART scene write IfcGeographicElement.
2. Revit moved terrain from IfcSite (2x3) to IfcGeographicElement (IFC4, by parameter). Allplan reads IfcSite as a DTM. Importers need both routes.
3. Georeferencing differs by schema. IFC4 uses IfcMapConversion with a scale (0.001 in the mm sample). IFC2x3 uses EPset_ psets with varying spelling. Site offsets double the shift in some files.
4. Real files break the rules. A Vectorworks IFC4X3 file had lowercase names, duplicate ids and bad GlobalIds. The buildingSMART scene uses `\X\27`, which our decoder leaves as it is.
5. A CC BY 4.0 corpus exists. The buildingSMART Certification-datasets repo has landscaping scenes in IFC2X3, IFC4 and IFC4X3_ADD2. The community repo has TIN, georeferencing and NURBS files. Schependomlaan is not usable.

Top 5 gaps:

1. `\X\`, `\S\` and `\P` string escapes are not decoded.
2. No ifcZIP, ifcXML or ifcJSON support.
3. No real-world corpus test. All fixtures are hand-written.
4. IfcOpeningElement voids are not cut.
5. IfcAdvancedBrep with B-splines and several swept solid kinds (revolved, swept disk, CSG) become boxes or are skipped.

## 14. Status after IOP-29

Corrections found when the corpus ran. The scene `Infra-Landscaping.ifc` (IFC4) has 8 IfcSite (1 COMPLEX, 7 PARTIAL), 76 IfcGeographicElement (14 terrain, 62 vegetation) and 101 products with a shape; the numbers 11 and 9 of section 12 are the numbers of types and materials. `\X\hh` was already decoded; `\S\` and `\P\` were not. Plants of the scene: 8 trees have a size, the other 54 vegetation parts (fruit, ground cover) stay meshes.

| Gap | Status |
|---|---|
| 1 escapes | done: `\S\`, `\P<A-I>\`, `\X\` is always Latin-1 (the code of a page is not used for it), partial units dropped |
| 2 ifcZIP, XML, JSON | ifcZIP done (read, write, caps); XML and JSON refused by name, backlog in `docs/interop.md` |
| 3 corpus | done (`ifc_corpus.rs`, 16 tests, pinned commits, hashes) |
| 4 nested sites | done: a layer per site when the file has several; assemblies follow their site |
| 5 openings | done: boolean cut, Reference not subtracted, failures kept as objects |
| 6 advanced breps | done through the STEP reader; cone, torus and seam-less cylinders of the Bentley samples are not cut by that reader (named in the report) |
| 7 revolved, swept disk, CSG | done, volumes against analytic values and IfcOpenShell |
| 8 TIN flags and holes | done (holes stay open, mesh); breaklines counted only; RC1 files without `PnIndex` read right |
| 9 map conversion edge cases | scale 0.001 tested; far projected coordinates done; factors of `IfcMapConversionScaled` noted, not applied; ePSet spellings tested; double site offset and feet CRS untested (no public file) |
| 10 large files | 24 MB synthetic file tested, cap message names `max_file_mb`; 200 MB and 2 GB not run |
| 11 grid and linear placement | grid placement done; linear placement counted in a note |
| 12 lower case, space after `=`, duplicates | done |
| 13 Allplan site face set | done: option `site_surface` |


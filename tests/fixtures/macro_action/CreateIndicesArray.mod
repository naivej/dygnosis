// Recursive include regression from Thomas Holden's DynareTransformationEngine.
// Source: tholden/DynareTransformationEngine, 46b96e359a7b985bb5de2c3ecf3cf7cf79833814,
// CreateIndicesArray.mod. GPL-3.0. Final array cast adapted for Dynare 7.2.
@#for CurrentIndex in 1 : SpatialPointsPerDimension
    @#define Indices = Indices + [ CurrentIndex ]
    @#define Depth = Depth + 1
    @#if Depth == SpatialDimensions
        @#define Name = ""
        @#for Dimension in 1 : SpatialDimensions
            @#define Name = Name + "_" + Numbers[Indices[Dimension]+1]
            @#define IndicesArray = IndicesArray + [ Indices[Dimension] ]
        @#endfor
        @#define IndicesStringArray = IndicesStringArray + [ Name ]
    @#else
        @#include "CreateIndicesArray.mod"
    @#endif
    @#define Depth = Depth - 1
    @#define Indices = (array) Indices[ 1:Depth ]
@#endfor

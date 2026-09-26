// compare: a new dimension is added and does not pair with another dimension
var y;
heterogeneity_dimension a, b;
model;
[name='anchor'] y = 0;
end;
model(heterogeneity=b);
[name='law'] y = 1;
[name='law'] y = 2;
end;

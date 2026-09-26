// compare: aggregate and heterogeneous rows with the same name and text do not pair
var y;
heterogeneity_dimension h;
model;
[name='anchor'] y = 0;
end;
model(heterogeneity=h);
[name='law'] y = 1;
end;

// compare: reorder inside one dimension, across two blocks
var y;
heterogeneity_dimension h;
var(heterogeneity=h) a, b;
model;
[name='anchor'] y = 0;
end;
model(heterogeneity=h);
[name='one'] a = 1;
end;
model(heterogeneity=h);
[name='two'] b = 2;
end;

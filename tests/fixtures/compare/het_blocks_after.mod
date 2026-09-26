// compare: indexes continue across two blocks of one dimension
var y;
heterogeneity_dimension h;
var(heterogeneity=h) a, b;
model;
[name='anchor'] y = 0;
end;
model(heterogeneity=h);
[name='first'] a = 1;
end;
model(heterogeneity=h);
[name='second'] b = 9;
end;

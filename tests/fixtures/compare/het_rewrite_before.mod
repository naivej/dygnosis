// compare: unique name pairs even when the text is far
var y;
heterogeneity_dimension h;
var(heterogeneity=h) a, b;
model;
[name='anchor'] y = 0;
end;
model(heterogeneity=h);
[name='euler'] a = 1;
end;

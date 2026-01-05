using MyApp.Shared;
using MyApp.Api;

namespace MyApp.Web
{
    public class WebController
    {
        public string Index()
        {
            var api = new ApiService();
            return $"web: {SharedLib.GetShared()}, {api.Handle()}";
        }
    }
}
